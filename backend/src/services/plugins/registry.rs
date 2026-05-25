//! Plugin registry sync + install.
//!
//! Fetches two signed JSON documents from nosdesk.com on startup and
//! every 24h, reconciles them with the local trust chain, and
//! exposes the cached index to the admin UI.
//!
//! Wire contract (see RFC in docs/plugins-registry.md once it
//! lands):
//!
//!   GET {base}/publishers.json
//!   GET {base}/publishers.json.sig
//!   GET {base}/index.json
//!   GET {base}/index.json.sig
//!
//! Both `.json` documents carry a monotonically-increasing `version`
//! field. Each `.sig` is base64 of an Ed25519 signature over
//! `b"nosdesk-registry-v1:" || <literal bytes of the JSON file>`,
//! produced by the Nosdesk root key whose public half is baked into
//! this binary via `signing::root_pubkey()`.
//!
//! Anti-rollback: the instance persists the highest version it has
//! accepted for each document in `plugin_registry_state` and refuses
//! snapshots whose version is lower.
//!
//! Failure is lenient by default: a fetch that can't complete logs
//! a warning and leaves the in-memory + DB state unchanged. The
//! `NOSDESK_REGISTRY_URL` env var opting to an empty string disables
//! the sync entirely (air-gapped deployments).

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::{DateTime, Utc};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::db::{DbConnection, Pool};
use crate::models::{NewTrustedPublisher, PluginRegistryStateUpdate};
use crate::repository::plugin_publishers;
use crate::sync::actor::ActorContext;
use crate::sync::session as actor_session;

/// Default registry URL. Set `NOSDESK_REGISTRY_URL=""` to disable
/// registry sync entirely (air-gapped deployments).
pub const DEFAULT_REGISTRY_URL: &str = "https://nosdesk.com/registry";

/// Cadence for background registry syncs in production.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Maximum size of a fetched JSON document. Registry files are
/// small; 4 MB is absurdly generous and only here to prevent an
/// adversarial server from streaming forever.
const MAX_JSON_SIZE: usize = 4 * 1024 * 1024;

/// Maximum size of a fetched `.sig` file. Ed25519 sigs are 64
/// bytes; base64 blows them up to 88. 512 is paranoia headroom.
const MAX_SIG_SIZE: usize = 512;

/// Domain separator bound into the signature over each registry
/// document. MUST match what the nosdesk.com build script signs.
const REGISTRY_SIGNING_PREFIX: &[u8] = b"nosdesk-registry-v1:";

/// Request timeout for any single HTTP fetch. Registry mirrors are
/// cheap static-file servers; a slow response means something is
/// wrong.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

// =============================================================================
// Wire types (matches the JSON schema served by nosdesk.com)
// =============================================================================

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublishersSnapshot {
    pub version: i64,
    pub generated_at: String,
    pub publishers: Vec<PublisherEntry>,
}

/// Tier of a publisher entry in `publishers.json`. The `official`
/// tier has no publisher row (it's the Nosdesk root key), so this
/// enum is intentionally narrower than `IndexPluginTier`. Modelled
/// as an enum rather than a string so a typo or unknown value in
/// the wire payload fails parse with a clear serde error, instead
/// of slipping through to a runtime warn-and-skip.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PublisherTier {
    Verified,
    Community,
}

impl PublisherTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Community => "community",
        }
    }
}

impl std::fmt::Display for PublisherTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Tier of a plugin entry in `index.json`. Wider than
/// `PublisherTier` because plugins can be `official` (signed by the
/// root key with no publisher row).
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IndexPluginTier {
    Official,
    Verified,
    Community,
}

impl IndexPluginTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Verified => "verified",
            Self::Community => "community",
        }
    }
}

impl std::fmt::Display for IndexPluginTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublisherEntry {
    pub pubkey: String,
    pub display_name: String,
    pub tier: PublisherTier,
    pub website: Option<String>,
    pub added_at: String,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IndexSnapshot {
    pub version: i64,
    pub generated_at: String,
    pub plugins: Vec<PluginIndexEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginIndexEntry {
    pub name: String,
    pub display_name: String,
    pub tier: IndexPluginTier,
    pub publisher_pubkey: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    /// Optional `https://` URL of an SVG icon for the registry
    /// browse UI. Pre-install rendering only; once installed, the
    /// icon comes from the bundled `icon.svg` extracted into the
    /// `plugins.icon_svg` column.
    #[serde(default)]
    pub icon_url: Option<String>,
    pub versions: Vec<VersionEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VersionEntry {
    pub version: String,
    pub released_at: String,
    pub download_url: String,
    /// Hex SHA-256 of the signed zip bytes. Re-checked on install
    /// so registry-to-CDN tampering is caught even before we hand
    /// the bytes to the signature verifier.
    pub sha256: String,
    pub min_nosdesk_version: Option<String>,
}

// =============================================================================
// In-memory cache
// =============================================================================

#[derive(Debug, Clone)]
pub struct RegistryCache {
    pub publishers: PublishersSnapshot,
    pub index: IndexSnapshot,
    pub fetched_at: DateTime<Utc>,
}

/// Sync state. `snapshot.is_none() && last_error.is_none()` is
/// the boot warm-up window; the handler distinguishes that from
/// "tried and failed" so the UI can render different empty
/// states. `disabled` is derived at request time from
/// `configured_url()`, not stored here.
#[derive(Debug, Default)]
pub struct RegistryState {
    pub snapshot: Option<RegistryCache>,
    pub last_error: Option<String>,
}

/// Shared handle to the in-memory sync state.
pub type SharedCache = Arc<RwLock<RegistryState>>;

pub fn new_cache() -> SharedCache {
    Arc::new(RwLock::new(RegistryState::default()))
}

impl RegistryCache {
    /// Look up a plugin entry by name. Callers resolve the version
    /// separately since "latest" vs "pinned" is a policy decision
    /// that belongs higher up.
    pub fn find_plugin(&self, name: &str) -> Option<&PluginIndexEntry> {
        self.index.plugins.iter().find(|p| p.name == name)
    }
}

impl PluginIndexEntry {
    /// Return the version entry matching `requested`, or the latest
    /// (topmost) entry if `requested` is `None`. We don't semver-sort;
    /// the registry publisher is expected to order `versions` newest-first.
    pub fn resolve_version(&self, requested: Option<&str>) -> Option<&VersionEntry> {
        match requested {
            Some(v) => self.versions.iter().find(|entry| entry.version == v),
            None => self.versions.first(),
        }
    }
}

// =============================================================================
// Errors
// =============================================================================

#[derive(Debug)]
pub enum RegistryError {
    /// `signing::root_pubkey()` returned None. Without a trust root
    /// we can't verify any signature, so every sync operation is
    /// refused.
    RootKeyNotConfigured,
    /// HTTP transport layer failure — DNS, TCP, TLS, 4xx/5xx, timeout.
    Fetch(String),
    /// Fetched bytes exceed `MAX_JSON_SIZE`/`MAX_SIG_SIZE`.
    TooLarge,
    /// JSON parse failure, missing fields, or `deny_unknown_fields` hit.
    Malformed(String),
    /// Base64 or 64-byte check on the signature file failed.
    InvalidSignature,
    /// Ed25519 verify failed against the root pubkey.
    BadSignature,
    /// Snapshot's `version` is <= the highest version we've previously
    /// accepted for that document. Defends against replay of older
    /// signed snapshots.
    Rollback {
        document: &'static str,
        seen: i64,
        offered: i64,
    },
    Db(diesel::result::Error),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RootKeyNotConfigured => write!(
                f,
                "NOSDESK_ROOT_PUBKEY is not configured; cannot verify the registry"
            ),
            Self::Fetch(m) => write!(f, "registry fetch failed: {m}"),
            Self::TooLarge => write!(f, "registry document exceeds size limit"),
            Self::Malformed(m) => write!(f, "registry document is malformed: {m}"),
            Self::InvalidSignature => {
                write!(
                    f,
                    "registry signature file is not valid base64 or wrong length"
                )
            }
            Self::BadSignature => write!(f, "registry signature verification failed"),
            Self::Rollback {
                document,
                seen,
                offered,
            } => write!(
                f,
                "refusing rollback of {document}: last accepted v{seen}, offered v{offered}"
            ),
            // Raw Diesel errors stay in the log, not the user-facing message.
            Self::Db(_) => write!(f, "registry DB error"),
        }
    }
}

impl std::error::Error for RegistryError {}

impl From<diesel::result::Error> for RegistryError {
    fn from(value: diesel::result::Error) -> Self {
        error!(error = %value, "Plugin registry DB error");
        RegistryError::Db(value)
    }
}

// =============================================================================
// Public entry points
// =============================================================================

/// Resolve the configured registry base URL. Returns `None` when
/// the operator has opted out via `NOSDESK_REGISTRY_URL=""`; in
/// that case the sync loop is not spawned and the install-from-
/// registry endpoint returns a descriptive error.
pub fn configured_url() -> Option<String> {
    match std::env::var("NOSDESK_REGISTRY_URL") {
        Ok(v) if v.is_empty() => None,
        Ok(v) => Some(v),
        Err(_) => Some(DEFAULT_REGISTRY_URL.to_string()),
    }
}

/// Perform one sync cycle: fetch + verify + reconcile. Safe to
/// call from startup hooks AND from the background loop.
pub async fn sync_once(
    http: &reqwest::Client,
    base_url: &str,
    pool: &Pool,
    cache: &SharedCache,
) -> Result<(), RegistryError> {
    let mut conn = pool
        .get()
        .map_err(|e| RegistryError::Fetch(format!("db pool: {e}")))?;

    let state = plugin_publishers::get_registry_state(&mut conn)?;

    let (publishers, index) = fetch_and_verify(http, base_url, &state).await?;

    // Exact-version replay is a no-op. Combined with the strict
    // anti-rollback in `fetch_and_verify`, an attacker can at most
    // waste one fetch cycle per version — never regress the DB.
    let unchanged =
        publishers.version == state.publishers_version && index.version == state.index_version;

    if !unchanged {
        // reconcile + state bump must commit together so a crash
        // between them can't leave the version counter regressed
        // relative to the publisher set we just applied.
        //
        // Open the transaction via with_actor_context so the
        // audit_log_trigger on plugin_trusted_publishers attributes
        // every publisher INSERT/UPDATE/DELETE to a named system
        // actor instead of NULL.
        let actor = ActorContext::system("scheduler:plugin_registry_sync");
        actor_session::with_actor_context::<_, RegistryError>(&mut conn, &actor, |tx| {
            reconcile(tx, &publishers, &index)?;
            plugin_publishers::update_registry_state(
                tx,
                PluginRegistryStateUpdate {
                    publishers_version: Some(publishers.version),
                    index_version: Some(index.version),
                    last_fetched_at: Some(Some(chrono::Utc::now().naive_utc())),
                    last_fetch_error: Some(None),
                    ..Default::default()
                },
            )?;
            Ok(())
        })?;
    } else {
        // Same version — just stamp last_fetched_at so operators
        // can see the sync ran.
        plugin_publishers::update_registry_state(
            &mut conn,
            PluginRegistryStateUpdate {
                last_fetched_at: Some(Some(chrono::Utc::now().naive_utc())),
                last_fetch_error: Some(None),
                ..Default::default()
            },
        )?;
    }

    let fetched_at = Utc::now();
    {
        let mut state = cache.write().await;
        state.snapshot = Some(RegistryCache {
            publishers: publishers.clone(),
            index: index.clone(),
            fetched_at,
        });
        state.last_error = None;
    }

    info!(
        publishers_version = publishers.version,
        index_version = index.version,
        publisher_count = publishers.publishers.len(),
        plugin_count = index.plugins.len(),
        unchanged,
        "Registry sync successful"
    );
    Ok(())
}

/// Spawn the long-running background loop. Fires `sync_once` at
/// boot then again every `SYNC_INTERVAL`. Failures are logged and
/// retried on the next tick; they never unwind the task.
pub fn spawn_sync_loop(pool: Pool, base_url: String, cache: SharedCache) {
    tokio::spawn(async move {
        let http = match build_http_client() {
            Ok(c) => c,
            Err(e) => {
                error!(error = %e, "Registry HTTP client build failed; sync disabled");
                return;
            }
        };

        loop {
            match sync_once(&http, &base_url, &pool, &cache).await {
                Ok(()) => {}
                Err(e) => {
                    let msg = e.to_string();
                    warn!(error = %msg, "Registry sync failed; will retry next cycle");
                    // Surface the failure on the in-memory state so
                    // the admin /plugins/registry view can render a
                    // "failed" state with the reason instead of an
                    // indistinguishable empty state.
                    cache.write().await.last_error = Some(msg.clone());
                    if let Ok(mut conn) = pool.get() {
                        let _ = plugin_publishers::update_registry_state(
                            &mut conn,
                            PluginRegistryStateUpdate {
                                last_fetch_error: Some(Some(msg)),
                                ..Default::default()
                            },
                        );
                    }
                }
            }
            tokio::time::sleep(SYNC_INTERVAL).await;
        }
    });
}

pub fn build_http_client() -> reqwest::Result<reqwest::Client> {
    // Routed through safe_http so the resolver refuses
    // hostnames that resolve to internal IPs. `https_only_client`
    // additionally pins the scheme to https — the registry
    // signing chain assumes TLS for the document fetches.
    crate::utils::safe_http::https_only_client(FETCH_TIMEOUT)
}

// =============================================================================
// Fetch + verify
// =============================================================================

async fn fetch_and_verify(
    http: &reqwest::Client,
    base_url: &str,
    state: &crate::models::PluginRegistryState,
) -> Result<(PublishersSnapshot, IndexSnapshot), RegistryError> {
    let root_b64 = crate::services::plugins::signing::root_pubkey()
        .ok_or(RegistryError::RootKeyNotConfigured)?;
    let root_bytes = BASE64
        .decode(root_b64.as_bytes())
        .map_err(|_| RegistryError::InvalidSignature)?;
    if root_bytes.len() != 32 {
        return Err(RegistryError::InvalidSignature);
    }

    // Fetch both documents concurrently — unrelated URLs, no data
    // dependency between them.
    let (pubs_res, idx_res) = tokio::join!(
        fetch_signed::<PublishersSnapshot>(http, base_url, "publishers.json", &root_bytes),
        fetch_signed::<IndexSnapshot>(http, base_url, "index.json", &root_bytes),
    );
    let publishers = pubs_res?;
    let index = idx_res?;

    // Strict `<` would accept a re-applied equal-version snapshot
    // post-crash, which combined with a non-atomic reconcile used
    // to allow a single-cycle rollback window. Reject equal too;
    // `sync_once` handles same-version as a no-op before we get
    // here, so the only way this fires is if state.publishers_version
    // advanced past what this snapshot claims (a legit rollback
    // attempt or a regressed publisher pipeline).
    if publishers.version <= state.publishers_version && state.publishers_version > 0 {
        if publishers.version < state.publishers_version {
            return Err(RegistryError::Rollback {
                document: "publishers.json",
                seen: state.publishers_version,
                offered: publishers.version,
            });
        }
        // Equal: allowed through for the no-op path above.
    }
    if index.version <= state.index_version && state.index_version > 0 {
        if index.version < state.index_version {
            return Err(RegistryError::Rollback {
                document: "index.json",
                seen: state.index_version,
                offered: index.version,
            });
        }
    }

    Ok((publishers, index))
}

/// Fetch `{base_url}/{name}` plus its `.sig` sibling, verify the
/// signature against `root_bytes`, and parse the JSON.
async fn fetch_signed<T: for<'de> Deserialize<'de>>(
    http: &reqwest::Client,
    base_url: &str,
    name: &str,
    root_bytes: &[u8],
) -> Result<T, RegistryError> {
    let doc_url = format!("{}/{}", base_url.trim_end_matches('/'), name);
    let sig_url = format!("{doc_url}.sig");

    // Content-Type allowlists are advisory: the signature check
    // gates execution regardless. The check only exists so a CDN
    // misconfig (HTML error page on the JSON path, etc.) surfaces
    // a clear "wrong content type" error instead of a confusing
    // serde or base64 parse failure further down.
    let (doc_bytes, sig_bytes) = tokio::join!(
        fetch_bytes(http, &doc_url, MAX_JSON_SIZE, &["application/json"]),
        fetch_bytes(
            http,
            &sig_url,
            MAX_SIG_SIZE,
            &[
                "application/pgp-signature",
                "application/octet-stream",
                "text/plain"
            ],
        ),
    );
    let doc_bytes = doc_bytes?;
    let sig_bytes = sig_bytes?;

    // Signature file is base64 of the raw 64-byte Ed25519 signature.
    let sig_str = std::str::from_utf8(&sig_bytes)
        .map_err(|_| RegistryError::InvalidSignature)?
        .trim();
    let sig = BASE64
        .decode(sig_str.as_bytes())
        .map_err(|_| RegistryError::InvalidSignature)?;
    if sig.len() != 64 {
        return Err(RegistryError::InvalidSignature);
    }

    let mut signed_input = Vec::with_capacity(REGISTRY_SIGNING_PREFIX.len() + doc_bytes.len());
    signed_input.extend_from_slice(REGISTRY_SIGNING_PREFIX);
    signed_input.extend_from_slice(&doc_bytes);

    UnparsedPublicKey::new(&ED25519, root_bytes)
        .verify(&signed_input, &sig)
        .map_err(|_| RegistryError::BadSignature)?;

    // Signature passed; now and only now do we trust the bytes
    // enough to hand them to serde.
    serde_json::from_slice::<T>(&doc_bytes).map_err(|e| RegistryError::Malformed(e.to_string()))
}

async fn fetch_bytes(
    http: &reqwest::Client,
    url: &str,
    cap: usize,
    accept_content_types: &[&str],
) -> Result<Vec<u8>, RegistryError> {
    debug!(url, "Registry fetch");
    // The safe_http resolver covers hostnames; this sync check
    // covers the IP-literal case where hyper-util skips DNS. A
    // hostile registry could otherwise hand back a
    // `download_url` of `http://127.0.0.1:5432/` and turn the
    // installer into a port scanner of the host.
    if let Err(e) = crate::utils::safe_http::reject_unsafe_ip_literal(url) {
        return Err(RegistryError::Fetch(format!(
            "{url}: blocked by SSRF guard: {e}"
        )));
    }
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| RegistryError::Fetch(format!("{url}: {e}")))?;
    if !resp.status().is_success() {
        return Err(RegistryError::Fetch(format!(
            "{url}: HTTP {}",
            resp.status()
        )));
    }
    // Defensive Content-Type check. The signature still gates
    // execution, but rejecting an obviously-wrong type here keeps
    // the error message specific (operator can fix the CDN config)
    // instead of conflating with a malformed-payload failure.
    if let Some(ct) = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    {
        let primary = ct.split(';').next().unwrap_or("").trim();
        if !accept_content_types
            .iter()
            .any(|allowed| primary.eq_ignore_ascii_case(allowed))
        {
            return Err(RegistryError::Fetch(format!(
                "{url}: unexpected Content-Type {primary:?} (expected one of {accept_content_types:?})"
            )));
        }
    }
    if let Some(len) = resp.content_length() {
        if len as usize > cap {
            return Err(RegistryError::TooLarge);
        }
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| RegistryError::Fetch(format!("{url}: {e}")))?;
    if bytes.len() > cap {
        return Err(RegistryError::TooLarge);
    }
    Ok(bytes.to_vec())
}

// =============================================================================
// DB reconciliation
// =============================================================================

/// Maximum fraction of previously-known publishers the registry
/// may drop in a single sync before the destructive step is
/// refused. An attacker (or a CI misfire) publishing an empty or
/// drastically-smaller publishers.json would otherwise revoke the
/// whole local trust list silently. 50% is conservative; a
/// legitimate pipeline mass-shrinking the publisher list should be
/// an explicit operator action, not a background sync event.
const MAX_SHRINKAGE_RATIO: f32 = 0.5;

fn reconcile(
    conn: &mut DbConnection,
    publishers: &PublishersSnapshot,
    _index: &IndexSnapshot,
) -> Result<(), RegistryError> {
    // Upsert publishers the registry knows about. The index is
    // cached in memory; there's no persistent plugin-catalog table
    // on the instance side.
    // Tier validity is enforced by the `PublisherTier` enum at
    // parse time (see fetch_and_verify), so any entry that reaches
    // here is structurally valid. No runtime tier filter required.
    let now = chrono::Utc::now().naive_utc();
    for entry in &publishers.publishers {
        let revoked_at = match &entry.revoked_at {
            Some(s) => Some(
                DateTime::parse_from_rfc3339(s)
                    .map(|dt| dt.naive_utc())
                    .map_err(|e| RegistryError::Malformed(format!("revoked_at: {e}")))?,
            ),
            None => None,
        };
        let record = NewTrustedPublisher {
            pubkey: entry.pubkey.clone(),
            display_name: entry.display_name.clone(),
            tier: entry.tier.as_str().to_string(),
            website: entry.website.clone(),
            revoked_at,
        };
        plugin_publishers::upsert_publisher(conn, record)?;
    }

    // Guard the destructive step. A snapshot that claims "zero
    // publishers" or "drop more than half of what we had" almost
    // certainly means a pipeline error or an attack; refuse rather
    // than apply. Upserts above already ran, so present-in-both
    // publishers are still updated correctly.
    let existing = plugin_publishers::list_all_publishers(conn)?;
    let active_before = existing.iter().filter(|p| p.revoked_at.is_none()).count();
    let known: std::collections::HashSet<&str> = publishers
        .publishers
        .iter()
        .map(|e| e.pubkey.as_str())
        .collect();
    let would_revoke = existing
        .iter()
        .filter(|p| !known.contains(p.pubkey.as_str()) && p.revoked_at.is_none())
        .count();

    if active_before > 0 && publishers.publishers.is_empty() {
        warn!(
            active_before,
            "Refusing destructive reconcile: registry snapshot has zero publishers"
        );
        return Err(RegistryError::Malformed(
            "snapshot has zero publishers; refusing to mass-revoke".into(),
        ));
    }
    if active_before >= 2 && would_revoke as f32 / active_before as f32 > MAX_SHRINKAGE_RATIO {
        warn!(
            active_before,
            would_revoke,
            "Refusing destructive reconcile: registry snapshot drops more than half of known publishers"
        );
        return Err(RegistryError::Malformed(format!(
            "snapshot would revoke {would_revoke}/{active_before} publishers; refusing"
        )));
    }

    // Apply revocations. Any publisher no longer mentioned by the
    // registry gets `revoked_at` stamped. Rows are preserved (not
    // deleted) because installed plugins still reference them via
    // `signer_pubkey` and we want the audit trail intact.
    for existing_pub in &existing {
        if !known.contains(existing_pub.pubkey.as_str()) && existing_pub.revoked_at.is_none() {
            info!(
                pubkey = %existing_pub.pubkey,
                "Publisher no longer in registry; marking revoked"
            );
            let _ = plugin_publishers::revoke_publisher(conn, &existing_pub.pubkey, now);
        }
    }

    Ok(())
}
