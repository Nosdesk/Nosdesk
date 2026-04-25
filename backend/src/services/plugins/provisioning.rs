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

/// Directory scanned for signed plugin zips on startup.
const PLUGINS_DIR: &str = "/app/plugins";

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
/// Arbitrary stable i64; only meaningful as a unique identifier.
const PROVISION_LOCK_KEY: i64 = 0x4e6f73_5052_5658; // "NosPRVX"

/// Scan `PLUGINS_DIR` and provision every `*.zip` file in it.
/// Two backend processes coming up at the same time (rolling
/// restart, debug + release running side by side) would otherwise
/// race on the same zip files; a session-scoped advisory lock
/// serialises the sweep. If the lock is already held, this call
/// returns early without scanning.
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

    let acquired: bool = match sql_query("SELECT pg_try_advisory_lock($1)")
        .bind::<BigInt, _>(PROVISION_LOCK_KEY)
        .get_result::<LockResult>(conn)
    {
        Ok(r) => r.pg_try_advisory_lock,
        Err(e) => {
            error!("Failed to acquire provisioning advisory lock: {e}");
            return vec![];
        }
    };
    if !acquired {
        info!("Another process holds the provisioning lock; skipping this sweep");
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
    let plugins_path = Path::new(PLUGINS_DIR);
    if !plugins_path.is_dir() {
        info!("Plugins directory does not exist, skipping provisioning");
        return vec![];
    }

    let entries = match fs::read_dir(plugins_path) {
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

    let created = results.iter().filter(|r| matches!(r, ProvisionResult::Created(_))).count();
    let updated = results.iter().filter(|r| matches!(r, ProvisionResult::Updated(_))).count();
    let unchanged = results.iter().filter(|r| matches!(r, ProvisionResult::Unchanged(_))).count();
    let failed = results.iter().filter(|r| matches!(r, ProvisionResult::Failed(_, _))).count();
    info!(
        "Plugin provisioning complete: {} created, {} updated, {} unchanged, {} failed",
        created, updated, unchanged, failed
    );
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

    match install::install_verified(conn, &files, signer, tier, options) {
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
            let entries = signing::read_archive(bytes)
                .map_err(|e| format!("Zip format error: {e}"))?;
            Ok((
                entries,
                trust::PluginSignerFields::dev_mode(),
                trust::ResolvedTier::Local,
            ))
        }
        Err(e) => Err(format!("Signature rejected: {e}")),
    }
}
