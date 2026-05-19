//! Plugin Provisioning Service
//!
//! Installs every signed zip at `/app/plugins/*.zip` on startup via
//! the shared `install::install_verified` core. The provisioner's
//! job is narrow: read a zip, verify its signature, resolve the
//! trust tier, and hand the verified bytes + signer fields to the
//! shared installer.
//!
//! `NOSDESK_DEV_MODE=1` (debug builds only) accepts unsigned zips
//! for local development. Release builds refuse the env var.

use std::env;
use std::fs;
use std::path::Path;

use tracing::{debug, error, info, warn};

use crate::db::DbConnection;
use crate::services::plugins::{install, signing, trust};
use crate::sync::actor::ActorContext;
use crate::sync::session as actor_session;

/// Directory scanned for signed plugin zips on startup.
/// Default plugins directory. The container image's compose mount
/// lands at `/app/plugins`. Local-host development without
/// containers can point at any other directory via
/// `NOSDESK_PLUGINS_DIR`.
const DEFAULT_PLUGINS_DIR: &str = "/app/plugins";

/// Resolve the configured plugins directory. Reads
/// `NOSDESK_PLUGINS_DIR` and falls back to the container default.
fn plugins_dir() -> std::path::PathBuf {
    std::env::var("NOSDESK_PLUGINS_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_PLUGINS_DIR))
}

/// Env var that bypasses signature verification. Debug builds only.
const DEV_MODE_ENV: &str = "NOSDESK_DEV_MODE";

fn dev_mode_enabled() -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    matches!(env::var(DEV_MODE_ENV), Ok(v) if v == "1" || v.eq_ignore_ascii_case("true"))
}

#[derive(Debug)]
pub enum ProvisionResult {
    Created(String),
    Updated(String),
    #[allow(dead_code)]
    Unchanged(String),
    Failed(String, String),
}

/// Postgres advisory-lock key for the provisioning sweep.
/// Stable i64 with no semantic meaning beyond being unique across
/// every other advisory lock the app uses. Hex spelling decodes to
/// the ASCII bytes "NosPRVX" so a `pg_locks` query is recognisable.
const PROVISION_LOCK_KEY: i64 = 0x4e6f73_5052_5658;

/// How many times to retry acquiring the lock before giving up.
/// Tuned to ride out a graceful shutdown of the previous process
/// (typically a few seconds) without delaying boot indefinitely
/// when the lock is genuinely contended.
const LOCK_ACQUIRE_ATTEMPTS: u32 = 5;
const LOCK_ACQUIRE_BACKOFF: std::time::Duration = std::time::Duration::from_secs(2);

/// Scan the plugins directory and provision every `*.zip` file in it.
/// Two backend processes coming up at the same time (rolling
/// restart, debug + release running side by side) would otherwise
/// race on the same zip files; a session-scoped advisory lock
/// serialises the sweep.
///
/// On contention we retry a bounded number of times rather than
/// skipping immediately. The skip path was the original behaviour
/// but it interacts poorly with rolling restarts: if the new
/// process boots while the old is mid-shutdown, the new one would
/// skip and never re-attempt, leaving any newly-dropped zip
/// unprovisioned until the next process restart.
pub fn provision_plugins(conn: &mut DbConnection) -> Vec<ProvisionResult> {
    use diesel::sql_query;
    use diesel::sql_types::BigInt;
    use diesel::QueryableByName;
    use diesel::RunQueryDsl;

    #[derive(QueryableByName)]
    struct LockResult {
        #[diesel(sql_type = diesel::sql_types::Bool)]
        pg_try_advisory_lock: bool,
    }

    let mut acquired = false;
    for attempt in 1..=LOCK_ACQUIRE_ATTEMPTS {
        let outcome = sql_query("SELECT pg_try_advisory_lock($1)")
            .bind::<BigInt, _>(PROVISION_LOCK_KEY)
            .get_result::<LockResult>(conn);
        match outcome {
            Ok(r) if r.pg_try_advisory_lock => {
                acquired = true;
                break;
            }
            Ok(_) => {
                if attempt < LOCK_ACQUIRE_ATTEMPTS {
                    info!(
                        attempt,
                        "Provisioning lock contended; retrying after {:?}", LOCK_ACQUIRE_BACKOFF
                    );
                    std::thread::sleep(LOCK_ACQUIRE_BACKOFF);
                }
            }
            Err(e) => {
                error!("Failed to acquire provisioning advisory lock: {e}");
                return vec![];
            }
        }
    }
    if !acquired {
        warn!(
            "Provisioning lock still held after {} attempts; skipping this sweep",
            LOCK_ACQUIRE_ATTEMPTS
        );
        return vec![];
    }

    let result = provision_plugins_locked(conn);

    // Release the session lock. If this fails the lock will be
    // released when the connection closes anyway, so just log.
    if let Err(e) = sql_query("SELECT pg_advisory_unlock($1)")
        .bind::<BigInt, _>(PROVISION_LOCK_KEY)
        .execute(conn)
    {
        warn!("Failed to release provisioning advisory lock: {e}");
    }

    result
}

fn provision_plugins_locked(conn: &mut DbConnection) -> Vec<ProvisionResult> {
    let plugins_path = plugins_dir();
    if !plugins_path.is_dir() {
        info!(
            path = %plugins_path.display(),
            "Plugins directory does not exist, skipping provisioning"
        );
        return vec![];
    }

    let entries = match fs::read_dir(&plugins_path) {
        Ok(e) => e,
        Err(e) => {
            error!("Failed to read plugins directory: {}", e);
            return vec![];
        }
    };

    let mut results = Vec::new();
    for entry in entries.flatten() {
        // `file_type()` on DirEntry does NOT follow symlinks; a
        // `plugin.zip -> /etc/passwd` is skipped as "not a regular
        // file" without ever being opened.
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                warn!("Skipping entry with unreadable file type: {e}");
                continue;
            }
        };
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("zip") {
            continue;
        }

        let label = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        results.push(provision_zip(conn, &path, &label));
    }

    let created = results
        .iter()
        .filter(|r| matches!(r, ProvisionResult::Created(_)))
        .count();
    let updated = results
        .iter()
        .filter(|r| matches!(r, ProvisionResult::Updated(_)))
        .count();
    let unchanged = results
        .iter()
        .filter(|r| matches!(r, ProvisionResult::Unchanged(_)))
        .count();
    let failed = results
        .iter()
        .filter(|r| matches!(r, ProvisionResult::Failed(_, _)))
        .count();
    info!(
        "Plugin provisioning complete: {} created, {} updated, {} unchanged, {} failed",
        created, updated, unchanged, failed
    );

    // Trust-tier inventory after the sweep settles. Logged as a
    // structured record so an operator can grep one line per boot
    // and see the distribution without scrolling the admin UI.
    // `dev_mode_count > 0` on a release build is a config smell;
    // `legacy_unsigned_count > 0` flags a migration straggler.
    // Failures here are non-fatal — the sweep itself already
    // succeeded, telemetry shouldn't gate startup.
    match crate::repository::plugins::signing_overview(conn) {
        Ok(o) => {
            let tiers: Vec<String> = o
                .by_trust_level
                .iter()
                .map(|t| format!("{}={}", t.trust_level, t.count))
                .collect();
            info!(
                total = o.total,
                dev_mode = o.dev_mode_count,
                legacy_unsigned = o.legacy_unsigned_count,
                tiers = %tiers.join(","),
                "Plugin trust-tier inventory"
            );
            if o.dev_mode_count > 0 && !cfg!(debug_assertions) {
                warn!(
                    count = o.dev_mode_count,
                    "Release build has dev-mode plugins installed; verify NOSDESK_DEV_MODE history"
                );
            }
            if o.legacy_unsigned_count > 0 {
                warn!(
                    count = o.legacy_unsigned_count,
                    "Plugins with no signer metadata detected; reinstall via signed path to clear"
                );
            }
        }
        Err(e) => warn!("Failed to compute trust-tier inventory: {e}"),
    }

    results
}

fn provision_zip(conn: &mut DbConnection, zip_path: &Path, label: &str) -> ProvisionResult {
    debug!("Provisioning plugin from: {}", zip_path.display());

    // Outer-file size cap. Matches the HTTP upload cap so both
    // entry points share a ceiling.
    let metadata = match fs::metadata(zip_path) {
        Ok(m) => m,
        Err(e) => return ProvisionResult::Failed(label.into(), format!("stat zip: {e}")),
    };
    if metadata.len() > signing::MAX_ARCHIVE_SIZE as u64 {
        return ProvisionResult::Failed(
            label.into(),
            format!("zip exceeds {} bytes", signing::MAX_ARCHIVE_SIZE),
        );
    }

    let bytes = match fs::read(zip_path) {
        Ok(b) => b,
        Err(e) => return ProvisionResult::Failed(label.into(), format!("read zip: {e}")),
    };

    let (files, signer, tier) = match resolve_signer(conn, &bytes, label) {
        Ok(r) => r,
        Err(msg) => return ProvisionResult::Failed(label.into(), msg),
    };

    let options = install::InstallOptions {
        source: "provisioned",
        installed_by: None,
        log_activity: false,
        provision_settings: true,
        skip_if_unchanged: true,
    };

    // System actor so the audit_log row carries an attribution
    // ("plugin_provisioner") rather than a NULL actor_uuid. The
    // install_verified transaction becomes a savepoint that
    // inherits the GUCs set by with_actor_context.
    let actor = ActorContext::system("plugin_provisioner");
    let result =
        actor_session::with_actor_context::<_, install::InstallError>(conn, &actor, |conn| {
            install::install_verified(conn, &files, signer, tier, options)
        });
    match result {
        Ok(install::InstallOutcome::Created(p)) => ProvisionResult::Created(p.name),
        Ok(install::InstallOutcome::Updated(p)) => ProvisionResult::Updated(p.name),
        Ok(install::InstallOutcome::Unchanged(p)) => ProvisionResult::Unchanged(p.name),
        Err(e) => ProvisionResult::Failed(label.into(), e.to_string()),
    }
}

/// Verify the zip's signature and resolve the trust tier, or fall
/// back to dev-mode unsigned handling in debug builds with
/// `NOSDESK_DEV_MODE=1`. Returns the entries to install, the
/// signer fields to stamp on the row, and the resolved tier (the
/// install pipeline's manifest validator needs it for author
/// binding). Dev-mode unsigned installs use `Local` tier — the
/// validator skips author checks for that tier.
fn resolve_signer(
    conn: &mut DbConnection,
    bytes: &[u8],
    label: &str,
) -> Result<
    (
        Vec<signing::ArchiveEntry>,
        trust::PluginSignerFields,
        trust::ResolvedTier,
    ),
    String,
> {
    match signing::verify_archive(bytes) {
        Ok(verified) => {
            let tier = trust::resolve(conn, &verified.envelope)
                .map_err(|e| format!("Publisher not trusted: {e}"))?;
            let signer = trust::PluginSignerFields::from_verified(&verified, &tier);
            Ok((verified.files, signer, tier))
        }
        Err(signing::SigningError::MissingSignature) if dev_mode_enabled() => {
            warn!(
                file = %label,
                "Provisioning unsigned zip because {}=1 is set. Never enable this in production.",
                DEV_MODE_ENV
            );
            let entries =
                signing::read_archive(bytes).map_err(|e| format!("Zip format error: {e}"))?;
            Ok((
                entries,
                trust::PluginSignerFields::dev_mode(),
                trust::ResolvedTier::Local,
            ))
        }
        Err(e) => Err(format!("Signature rejected: {e}")),
    }
}
