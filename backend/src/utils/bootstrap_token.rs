//! Bootstrap token: gates `POST /api/auth/setup/admin` against
//! network attackers who might race the legitimate operator on
//! first boot.
//!
//! Lifecycle. At server startup, if zero users exist, write a
//! random 32-byte base64 token to `${UPLOAD_DIR}/bootstrap.token`
//! (mode 0600) and log its location. The operator retrieves it
//! via shell access (`docker compose exec backend cat ...`) and
//! supplies it as `Authorization: Bearer <token>` on the
//! setup-admin request. After a successful setup the file is
//! removed, which together with the existing `count(users) > 0`
//! short-circuit makes the endpoint inert.
//!
//! If users already exist at startup, any stale token file is
//! deleted (defence against a restored backup leaving an old
//! token behind on disk).

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;

use crate::db::DbConnection;

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
    let upload_dir =
        std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "/app/uploads".to_string());
    PathBuf::from(upload_dir).join("bootstrap.token")
}

/// Idempotent: call at startup once the DB pool is ready. Writes
/// a fresh token if no users exist and no file is present;
/// removes the file if users exist.
pub fn reconcile(conn: &mut DbConnection) -> Result<()> {
    if has_any_user(conn)? {
        delete_token_file();
        return Ok(());
    }
    let path = token_file_path();
    if path.exists() {
        tracing::info!(
            "bootstrap token already present at {}",
            path.display()
        );
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let token = generate_token();
    write_token_file(&path, &token)?;
    tracing::warn!(
        path = %path.display(),
        "First-boot bootstrap token written. Retrieve with: `cat {}` and pass it as `Authorization: Bearer <token>` on POST /api/auth/setup/admin",
        path.display()
    );
    Ok(())
}

/// Returns `Ok(())` when the provided token matches the on-disk
/// file. Returns an error otherwise. Comparison is constant-time
/// to avoid leaking the prefix via timing.
pub fn verify(provided: &str) -> Result<()> {
    let path = token_file_path();
    let mut on_disk = String::new();
    let mut f = fs::File::open(&path)
        .map_err(|_| anyhow!("bootstrap token not present; setup is closed"))?;
    f.read_to_string(&mut on_disk)
        .with_context(|| "reading bootstrap token")?;
    let on_disk = on_disk.trim();
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

    fn with_temp_upload_dir<F: FnOnce()>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("UPLOAD_DIR");
        std::env::set_var("UPLOAD_DIR", dir.path());
        f();
        match prev {
            Some(v) => std::env::set_var("UPLOAD_DIR", v),
            None => std::env::remove_var("UPLOAD_DIR"),
        }
    }

    #[test]
    fn token_file_path_honours_upload_dir() {
        with_temp_upload_dir(|| {
            let p = token_file_path();
            let upload = std::env::var("UPLOAD_DIR").unwrap();
            assert!(p.starts_with(&upload));
            assert!(p.ends_with("bootstrap.token"));
        });
    }

    #[test]
    fn verify_rejects_missing_file() {
        with_temp_upload_dir(|| {
            let err = verify("anything").unwrap_err().to_string();
            assert!(err.contains("not present"));
        });
    }

    #[test]
    fn verify_accepts_matching_token_and_rejects_others() {
        with_temp_upload_dir(|| {
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
        with_temp_upload_dir(|| {
            let path = token_file_path();
            write_token_file(&path, "tok").unwrap();
            assert!(path.exists());
            consume();
            assert!(!path.exists());
            consume();
        });
    }
}
