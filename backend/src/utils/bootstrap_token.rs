//! Bootstrap token: gates `POST /api/auth/setup/admin` against
//! network attackers who might race the legitimate operator on
//! first boot.
//!
//! Lifecycle. At server startup, if zero users exist, write a
//! random 32-byte base64 token to
//! `${NOSDESK_STATE_DIR:-/app/state}/bootstrap.token` (mode 0600) and
//! log a clickable setup URL alongside the bare token. The token lives
//! in a dedicated state directory, never under the user-content upload
//! path, so a path-traversal bug in an upload handler cannot read it
//! (PocketBase / GitLab convention). The operator either:
//!   1. clicks the logged URL (works on-host with default
//!      `FRONTEND_URL`, or via reverse proxy when `FRONTEND_URL`
//!      is configured), or
//!   2. pastes the bare token into the setup form's manual entry.
//! The URL flow is the Pocketbase v0.23 pattern, motivated by the
//! same network-race CVE class we're defending against
//! (CVE-2024-31218).
//!
//! Tokens expire `BOOTSTRAP_TOKEN_TTL_SECONDS` after the file's
//! mtime (default 3600 = 60 minutes). An expired token is treated
//! as absent: `verify()` rejects it, `reconcile()` deletes the
//! file and mints a fresh one. An operator who walks away for
//! lunch comes back to a fresh token after restart; an operator
//! actively setting up doesn't see the token rotate mid-flow
//! (because we re-use the existing non-expired file on boot
//! rather than minting unconditionally).
//!
//! After a successful setup the file is removed, which together
//! with the existing `count(users) > 0` short-circuit makes the
//! endpoint inert.
//!
//! If users already exist at startup, any stale token file is
//! deleted (defence against a restored backup leaving an old
//! token behind on disk).

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;

use crate::db::DbConnection;
use crate::middleware::DeploymentMode;

/// Default TTL — 30 minutes (matching PocketBase's post-CVE-2024-31218
/// installer-token window). Long enough for an operator to complete
/// first-boot setup, short enough that an exfiltrated startup-log line
/// goes stale quickly. Override with `BOOTSTRAP_TOKEN_TTL_SECONDS` for
/// slow deployments.
const DEFAULT_TTL_SECONDS: u64 = 30 * 60;

fn ttl() -> Duration {
    let secs = std::env::var("BOOTSTRAP_TOKEN_TTL_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TTL_SECONDS);
    Duration::from_secs(secs)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

const TOKEN_BYTES: usize = 32;

pub fn token_file_path() -> PathBuf {
    // Dedicated state directory, deliberately NOT under UPLOAD_DIR: the
    // uploads tree is the user-content blast radius, so isolating the
    // token keeps an upload path-traversal bug from reaching it.
    let state_dir = std::env::var("NOSDESK_STATE_DIR").unwrap_or_else(|_| "/app/state".to_string());
    PathBuf::from(state_dir).join("bootstrap.token")
}

/// Returns `true` if the file's mtime is older than the
/// configured TTL. Missing-file returns `true` so callers can
/// treat absent and expired identically.
fn is_expired(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return true;
    };
    let Ok(modified) = meta.modified() else {
        // Filesystems without mtime support (very rare): err on
        // the side of caution and treat as expired so the token
        // can't outlive the boot.
        return true;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|elapsed| elapsed > ttl())
        .unwrap_or(false)
}

/// Idempotent: call at startup once the DB pool is ready.
///
/// In `Hosted` mode this is a no-op beyond deleting any stale token
/// file: there is no self-serve bootstrap, so no token is minted.
///
/// In `SelfHosted` mode, behaviour given the users table:
///   - non-empty → delete any stale token file, return.
///   - empty + file exists + not expired → log the setup URL
///     again (so a restart surfaces it without rotating the
///     value out from under an in-progress operator).
///   - empty + file exists + expired → delete + mint fresh.
///   - empty + file missing → mint fresh.
pub fn reconcile(conn: &mut DbConnection, mode: DeploymentMode) -> Result<()> {
    if mode == DeploymentMode::Hosted {
        // No bootstrap path in hosted: the control plane provisions
        // the first admin per tenant, so there is no URL flow to mint.
        // Clear any token a prior self-hosted run or restored backup
        // left on disk.
        delete_token_file();
        return Ok(());
    }
    if has_any_user(conn)? {
        delete_token_file();
        return Ok(());
    }
    let path = token_file_path();
    if path.exists() && !is_expired(&path) {
        let token = read_token_file(&path)?;
        log_setup_line(&token);
        return Ok(());
    }
    if path.exists() {
        // Expired. Remove before re-minting so write_token_file's
        // create_new(true) doesn't trip on the leftover.
        tracing::info!("bootstrap token expired; minting fresh");
        delete_token_file();
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let token = generate_token();
    write_token_file(&path, &token)?;
    log_setup_line(&token);
    Ok(())
}

/// Spawn a background task that logs once when an unconsumed bootstrap
/// token reaches its TTL, then deletes the stale file.
///
/// No cancellation channel is needed: [`consume`] deletes the token
/// file on a successful setup, so when this task wakes it simply checks
/// whether the file is still present and expired. If setup already
/// happened (or the token was never minted, e.g. hosted mode), the file
/// is absent and the task is a no-op. Call once at startup, after
/// [`reconcile`].
pub fn spawn_expiry_logger() {
    tokio::spawn(async move {
        let path = token_file_path();
        let Ok(meta) = fs::metadata(&path) else {
            return; // no token on disk (already consumed, or hosted)
        };
        let Ok(modified) = meta.modified() else {
            return;
        };
        let remaining = (modified + ttl())
            .duration_since(SystemTime::now())
            .unwrap_or(Duration::ZERO);
        tokio::time::sleep(remaining).await;

        // consume() deletes the file on success, so a still-present,
        // expired file means the operator never completed setup.
        if path.exists() && is_expired(&path) {
            tracing::warn!(
                "bootstrap token expired without being consumed; restart the \
                 backend to mint a fresh one, or use the CLI \
                 (`nosdesk-cli admin create`) or INITIAL_ADMIN_* env paths"
            );
            delete_token_file();
        }
    });
}

/// Compose the setup URL the operator should visit. Uses
/// `FRONTEND_URL` when set (reverse-proxy operators), falls back
/// to `http://localhost:8080` for on-host docker compose users.
/// Operators behind a reverse proxy who haven't configured
/// `FRONTEND_URL` can either set it and restart, or substitute
/// their hostname in the URL; the token is right there.
fn setup_url(token: &str) -> String {
    let base = std::env::var("FRONTEND_URL")
        .ok()
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| "http://localhost:8080".to_string());
    // `/onboarding` is the SPA route that hosts the initial-admin
    // form; see `frontend/src/router/index.ts`. Keep this string in
    // sync with that router entry.
    format!("{base}/onboarding?token={token}")
}

/// The current bootstrap token and its setup URL, if an unexpired token
/// exists on disk. `None` when setup is already complete (no token file)
/// or the token has expired. Backs the `nosdesk-cli setup-token` command
/// so an operator can fetch the value on demand (e.g. after a detached
/// `docker compose up -d`) without grepping the logs.
pub fn current_token_and_url() -> Option<(String, String)> {
    let path = token_file_path();
    if !path.exists() || is_expired(&path) {
        return None;
    }
    let token = read_token_file(&path).ok()?;
    if token.is_empty() {
        return None;
    }
    let url = setup_url(&token);
    Some((token, url))
}

/// Emit the operator-facing first-boot banner.
///
/// Printed as a bordered block straight to stderr rather than through
/// `tracing`, for two reasons: (1) it shows regardless of `RUST_LOG`
/// (an operator who set `RUST_LOG=error` still needs the setup URL),
/// and (2) it's free of the per-line log prefix (timestamp / target /
/// level), so the URL and token are clean to copy-paste. This mirrors
/// the installer banners PocketBase / Gitea print on first run, and is
/// far easier to spot in `docker compose logs` than a single log line
/// lost in the startup noise. A structured `tracing::info!` is also
/// emitted so log aggregators still capture the event.
fn log_setup_line(token: &str) {
    let url = setup_url(token);
    let ttl_minutes = ttl().as_secs() / 60;
    let rule = "  ──────────────────────────────────────────────────────────────";

    let banner = [
        String::new(),
        rule.to_string(),
        "  Nosdesk · first-run setup".to_string(),
        rule.to_string(),
        String::new(),
        "  Open this URL to create your administrator account:".to_string(),
        String::new(),
        format!("      {url}"),
        String::new(),
        "  …or paste this token into the setup form:".to_string(),
        String::new(),
        format!("      {token}"),
        String::new(),
        format!("  Expires in {ttl_minutes} min. Behind a reverse proxy? Set FRONTEND_URL"),
        "  and restart, or swap the host in the URL above.".to_string(),
        String::new(),
        "  Need it again later? Run:".to_string(),
        "      docker compose exec backend nosdesk-cli setup-token".to_string(),
        String::new(),
    ]
    .join("\n");
    eprintln!("{banner}");

    // Structured mirror for log aggregators / non-interactive capture.
    tracing::info!(%url, expires_in_min = ttl_minutes, "bootstrap setup URL minted");
}

fn read_token_file(path: &Path) -> Result<String> {
    let mut on_disk = String::new();
    let mut f = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    f.read_to_string(&mut on_disk)
        .with_context(|| "reading bootstrap token")?;
    Ok(on_disk.trim().to_string())
}

/// Returns `Ok(())` when the provided token matches the on-disk
/// file AND the file is within its TTL. Returns an error
/// otherwise. Comparison is constant-time to avoid leaking the
/// prefix via timing. Expiry is checked before the read so an
/// expired file is treated as if absent (the operator's
/// next request gets the same "setup is closed" error as a
/// post-setup attempt).
pub fn verify(provided: &str) -> Result<()> {
    let path = token_file_path();
    if !path.exists() {
        return Err(anyhow!("bootstrap token not present; setup is closed"));
    }
    if is_expired(&path) {
        return Err(anyhow!(
            "bootstrap token expired; restart the backend to mint a fresh one"
        ));
    }
    let on_disk = read_token_file(&path)?;
    let provided = provided.trim();
    if on_disk.is_empty() {
        return Err(anyhow!("bootstrap token file is empty"));
    }
    if !constant_time_eq(on_disk.as_bytes(), provided.as_bytes()) {
        return Err(anyhow!("bootstrap token mismatch"));
    }
    Ok(())
}

/// Best-effort removal after a successful setup. Logs but doesn't
/// fail the surrounding flow if the unlink fails: the count gate
/// (`count(users) > 0`) is the load-bearing check; this is the
/// belt to its braces.
pub fn consume() {
    delete_token_file();
}

fn delete_token_file() {
    let path = token_file_path();
    match fs::remove_file(&path) {
        Ok(()) => tracing::info!(path = %path.display(), "bootstrap token removed"),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(
            path = %path.display(),
            error = %e,
            "failed to remove bootstrap token"
        ),
    }
}

fn write_token_file(path: &std::path::Path, token: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    f.write_all(token.as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    f.write_all(b"\n").ok();
    Ok(())
}

fn generate_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn has_any_user(conn: &mut DbConnection) -> Result<bool> {
    use crate::schema::users;
    use diesel::dsl::count_star;
    use diesel::prelude::*;
    let n: i64 = users::table
        .select(count_star())
        .first(conn)
        .with_context(|| "counting users")?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_temp_state_dir<F: FnOnce()>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("NOSDESK_STATE_DIR");
        std::env::set_var("NOSDESK_STATE_DIR", dir.path());
        f();
        match prev {
            Some(v) => std::env::set_var("NOSDESK_STATE_DIR", v),
            None => std::env::remove_var("NOSDESK_STATE_DIR"),
        }
    }

    #[test]
    fn token_file_path_honours_state_dir() {
        with_temp_state_dir(|| {
            let p = token_file_path();
            let state = std::env::var("NOSDESK_STATE_DIR").unwrap();
            assert!(p.starts_with(&state));
            assert!(p.ends_with("bootstrap.token"));
        });
    }

    #[test]
    fn verify_rejects_missing_file() {
        with_temp_state_dir(|| {
            let err = verify("anything").unwrap_err().to_string();
            assert!(err.contains("not present"));
        });
    }

    #[test]
    fn verify_accepts_matching_token_and_rejects_others() {
        with_temp_state_dir(|| {
            let path = token_file_path();
            write_token_file(&path, "the-real-token").unwrap();
            verify("the-real-token").unwrap();
            verify("the-real-token\n").unwrap();
            verify(" the-real-token ").unwrap();
            assert!(verify("the-wrong-token").is_err());
            assert!(verify("").is_err());
        });
    }

    #[test]
    fn consume_removes_the_file_and_is_idempotent() {
        with_temp_state_dir(|| {
            let path = token_file_path();
            write_token_file(&path, "tok").unwrap();
            assert!(path.exists());
            consume();
            assert!(!path.exists());
            consume();
        });
    }

    #[test]
    fn verify_rejects_expired_file() {
        with_temp_state_dir(|| {
            // Configure a 1-second TTL so we don't have to sleep
            // an hour. The env-var read happens inside `ttl()`,
            // so updating it mid-test is fine.
            std::env::set_var("BOOTSTRAP_TOKEN_TTL_SECONDS", "1");
            let path = token_file_path();
            write_token_file(&path, "tok").unwrap();

            // Backdate the file's mtime so it appears > 1s old.
            // `File::set_times` lands on stable; no trait import
            // needed.
            let f = fs::File::options().write(true).open(&path).unwrap();
            let backdate = SystemTime::now() - Duration::from_secs(10);
            let times = fs::FileTimes::new().set_modified(backdate);
            f.set_times(times).unwrap();
            drop(f);

            let err = verify("tok").unwrap_err().to_string();
            assert!(err.contains("expired"), "got: {err}");
            std::env::remove_var("BOOTSTRAP_TOKEN_TTL_SECONDS");
        });
    }

    #[test]
    fn reconcile_clears_token_and_does_not_mint_in_hosted_mode() {
        with_temp_state_dir(|| {
            let path = token_file_path();
            write_token_file(&path, "tok").unwrap();
            assert!(path.exists());

            // Hosted short-circuits before any DB access, so the
            // connection is never queried; it just satisfies the
            // signature.
            let mut conn = crate::test_helpers::setup_test_connection();
            reconcile(&mut conn, DeploymentMode::Hosted).expect("hosted reconcile");

            assert!(
                !path.exists(),
                "hosted reconcile must clear a stale token and mint nothing"
            );
        });
    }

    #[test]
    fn setup_url_uses_frontend_url_when_set_else_localhost() {
        with_temp_state_dir(|| {
            std::env::remove_var("FRONTEND_URL");
            assert_eq!(
                setup_url("abc"),
                "http://localhost:8080/onboarding?token=abc"
            );

            // Trailing slash on FRONTEND_URL must not produce a
            // double-slash in the result.
            std::env::set_var("FRONTEND_URL", "https://desk.example.com/");
            assert_eq!(
                setup_url("abc"),
                "https://desk.example.com/onboarding?token=abc"
            );
            std::env::remove_var("FRONTEND_URL");
        });
    }
}
