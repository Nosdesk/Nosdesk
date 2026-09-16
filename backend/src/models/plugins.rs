use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ===== PLUGIN SYSTEM TYPES =====

/// Plugin trust level
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum PluginTrustLevel {
    Official,
    Verified,
    #[default]
    Community,
}

impl std::fmt::Display for PluginTrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginTrustLevel::Official => write!(f, "official"),
            PluginTrustLevel::Verified => write!(f, "verified"),
            PluginTrustLevel::Community => write!(f, "community"),
        }
    }
}

impl std::str::FromStr for PluginTrustLevel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "official" => Ok(PluginTrustLevel::Official),
            "verified" => Ok(PluginTrustLevel::Verified),
            "community" => Ok(PluginTrustLevel::Community),
            _ => Err(anyhow::anyhow!("Unknown trust level: {}", s)),
        }
    }
}

/// Installed plugin
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugins)]
pub struct Plugin {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub manifest: serde_json::Value,
    pub trust_level: String,
    pub installed_by: Option<Uuid>,
    pub installed_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub bundle_hash: Option<String>,
    pub bundle_size: Option<i32>,
    pub bundle_uploaded_at: Option<NaiveDateTime>,
    pub source: String,
    /// Base64 Ed25519 pubkey that signed this bundle. The current
    /// install pipeline always populates this (every install path
    /// goes through signature verification), so production rows
    /// have `Some`; the column is `Option` only to tolerate
    /// pre-signing-system rows in upgraded databases.
    pub signer_pubkey: Option<String>,
    /// Which authority chain recognised this signer: `nosdesk-root`
    /// | `verified-publisher` | `community-publisher` | `local` |
    /// `dev`. See `services::plugins::signing::sources`.
    pub signer_source: Option<String>,
    /// Full signature envelope captured at install time for audit.
    pub signature_metadata: Option<serde_json::Value>,
    /// Validated `icon.svg` bytes extracted from the signed zip at
    /// install time. Served verbatim from `GET /api/plugins/{uuid}/icon`.
    pub icon_svg: Option<Vec<u8>>,
    /// Lifecycle state. Stringly-typed in the DB (VARCHAR with a
    /// CHECK constraint) but parsed into the typed `PluginState`
    /// enum on read; consumers match exhaustively, eliminating
    /// the typo class that the constants module was prone to.
    pub state: PluginState,
    /// Bundle bytes stored inline. Replaces the previous on-disk
    /// uploads-volume staging so install becomes a single
    /// transactional write (DB row + bundle bytes commit together
    /// or both roll back). NULL only on legacy rows installed
    /// before this column existed; reinstall populates it. Capped
    /// at `install::MAX_BUNDLE_SIZE` (500 KB).
    pub bundle_js: Option<Vec<u8>>,
    pub workspace_id: i32,
    /// The exact permission set an admin consented to (JSON array of permission
    /// strings), or `None` before first consent. A later version whose
    /// permissions aren't a subset of this requires re-consent.
    pub consented_permissions: Option<serde_json::Value>,
    pub consented_at: Option<NaiveDateTime>,
    pub consented_by: Option<Uuid>,
}

impl Plugin {
    /// True when the plugin is in the `installed` state (active +
    /// loaded). Replaces the old `enabled` boolean for callers that
    /// only need a yes/no view.
    pub fn is_active(&self) -> bool {
        matches!(self.state, PluginState::Installed)
    }

    /// The permission set the admin consented to, as a `Vec<String>`. Empty when
    /// no consent recorded yet (parses the `consented_permissions` JSON array).
    pub fn consented_permission_set(&self) -> Vec<String> {
        self.consented_permissions
            .as_ref()
            .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            .unwrap_or_default()
    }

    /// The effective granted permission set for runtime enforcement: the recorded
    /// consented set when consent exists, else (legacy rows installed before the
    /// consent gate) the manifest's requested set. Empty on a manifest parse
    /// failure — fail closed. `consented_permissions == Some([])` is an explicit
    /// grant of nothing, distinct from `None` (no consent recorded), so the
    /// fallback keys off `is_none()`, not emptiness.
    pub fn effective_permission_set(&self) -> Vec<String> {
        match &self.consented_permissions {
            Some(v) => serde_json::from_value::<Vec<String>>(v.clone()).unwrap_or_default(),
            None => self
                .parse_manifest()
                .map(|m| m.permissions.iter().map(|p| p.as_string()).collect())
                .unwrap_or_default(),
        }
    }

    /// Whether the plugin's effective (consented) grant includes `needed`. The
    /// server-side authorization gate for plugin-owned data endpoints (storage,
    /// collections). Exact-string match, so it's for the fixed capability
    /// permissions (`storage:plugin`, `collection:read`, ...), not `network:*`
    /// host patterns (the proxy enforces those against the target host).
    pub fn has_effective_permission(&self, needed: &str) -> bool {
        self.effective_permission_set().iter().any(|p| p == needed)
    }
}

/// Lifecycle state of a plugin row. Stored as a `VARCHAR(32)` in
/// `plugins.state` with a CHECK constraint enforcing the allowlist;
/// the typed enum here is the canonical in-memory representation.
/// Custom Diesel `ToSql<Text>` / `FromSql<Text>` impls handle the
/// wire conversion. Adding a new variant means migrating the DB
/// CHECK constraint AND extending the exhaustive matches that
/// fall out elsewhere; the compiler points at every site.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::AsExpression,
    diesel::FromSqlRow,
    serde::Serialize,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    /// Active. Bundle is served, components render, events dispatch.
    Installed,
    /// Admin paused. Bundle is NOT served, components don't render,
    /// but the row + plugin_data are intact and a flip back to
    /// `Installed` restores everything.
    Disabled,
    /// Trust-chain failure (signer revoked, signature mismatched on
    /// re-check). Refused for new use; existing data preserved for
    /// audit. Triggered by background revocation sweeps; never set
    /// by user action.
    Quarantined,
    /// Plugin was uninstalled via a manifest declaring
    /// `lifecycle.on_uninstall = preserve`. The row + plugin_data
    /// + collection rows are kept so a future reinstall of the same
    /// plugin name reattaches the data automatically. Bundle is
    /// removed from disk.
    Uninstalled,
    /// Installed but not yet consented to: the row exists and the bundle is
    /// stored, but it is NOT served (the loader serves only `Installed`), so this
    /// state is inert until an admin approves the requested permission scope,
    /// which advances it to `Installed`. Untrusted tiers (verified / community)
    /// land here at install; trusted tiers (official / local) auto-advance.
    AwaitingConsent,
}

impl PluginState {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            PluginState::Installed => "installed",
            PluginState::Disabled => "disabled",
            PluginState::Quarantined => "quarantined",
            PluginState::Uninstalled => "uninstalled",
            PluginState::AwaitingConsent => "awaiting_consent",
        }
    }

    pub fn from_db_str(s: &str) -> Result<Self, String> {
        match s {
            "installed" => Ok(PluginState::Installed),
            "disabled" => Ok(PluginState::Disabled),
            "quarantined" => Ok(PluginState::Quarantined),
            "uninstalled" => Ok(PluginState::Uninstalled),
            "awaiting_consent" => Ok(PluginState::AwaitingConsent),
            other => Err(format!("unknown plugin state {other:?}")),
        }
    }
}

impl std::fmt::Display for PluginState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db_str())
    }
}

impl<'de> serde::Deserialize<'de> for PluginState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        PluginState::from_db_str(&s).map_err(serde::de::Error::custom)
    }
}

impl diesel::serialize::ToSql<diesel::sql_types::Text, diesel::pg::Pg> for PluginState {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        <str as diesel::serialize::ToSql<diesel::sql_types::Text, diesel::pg::Pg>>::to_sql(
            self.as_db_str(),
            &mut out.reborrow(),
        )
    }
}

impl diesel::deserialize::FromSql<diesel::sql_types::Text, diesel::pg::Pg> for PluginState {
    fn from_sql(
        bytes: <diesel::pg::Pg as diesel::backend::Backend>::RawValue<'_>,
    ) -> diesel::deserialize::Result<Self> {
        let s = <String as diesel::deserialize::FromSql<
            diesel::sql_types::Text,
            diesel::pg::Pg,
        >>::from_sql(bytes)?;
        PluginState::from_db_str(&s).map_err(|e| e.into())
    }
}

/// New plugin for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugins)]
pub struct NewPlugin {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub manifest: serde_json::Value,
    /// Initial lifecycle state: `Installed` for auto-consented (official / local)
    /// tiers, `AwaitingConsent` for the tiers that require admin consent.
    pub state: PluginState,
    pub trust_level: String,
    pub installed_by: Option<Uuid>,
    pub source: String,
    pub signer_pubkey: Option<String>,
    pub signer_source: Option<String>,
    pub signature_metadata: Option<serde_json::Value>,
    pub icon_svg: Option<Vec<u8>>,
    /// Consent recorded at install for auto-consented tiers (the manifest's
    /// permission set); `None` when the plugin lands in `AwaitingConsent`.
    pub consented_permissions: Option<serde_json::Value>,
    pub consented_at: Option<NaiveDateTime>,
    pub consented_by: Option<Uuid>,
}

/// Plugin update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugins)]
pub struct PluginUpdate {
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub manifest: Option<serde_json::Value>,
    pub state: Option<PluginState>,
    pub trust_level: Option<String>,
    pub signer_pubkey: Option<String>,
    pub signer_source: Option<String>,
    pub signature_metadata: Option<serde_json::Value>,
    /// `Some(Some(bytes))` writes the icon, `Some(None)` clears it,
    /// `None` leaves it alone. Distinct from the other signer
    /// fields' `Option<T>` because clearing-to-NULL on update is
    /// realistic here (a new plugin version might drop its icon).
    pub icon_svg: Option<Option<Vec<u8>>>,
    /// Consent bookkeeping, kept in lockstep with the served manifest by the
    /// update path. `None` leaves the column alone (used when an update requires
    /// re-consent — the admin's approval re-records it against the new manifest).
    pub consented_permissions: Option<serde_json::Value>,
    pub consented_at: Option<NaiveDateTime>,
    pub consented_by: Option<Uuid>,
}

/// Plugin bundle update changeset. `bundle_js` carries the raw
/// bytes; `bundle_hash`/`size`/`uploaded_at` are denormalised
/// metadata kept in sync. All four fields are written in the
/// same row update so they can't drift.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = crate::schema::plugins)]
pub struct PluginBundleUpdate {
    pub bundle_js: Option<Vec<u8>>,
    pub bundle_hash: Option<String>,
    pub bundle_size: Option<i32>,
    pub bundle_uploaded_at: Option<NaiveDateTime>,
}

/// Publisher whose Ed25519 pubkey is trusted to sign `verified` or
/// `community` tier plugins. Populated from the signed nosdesk.com
/// keylist; revocation is expressed by setting `revoked_at`.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugin_trusted_publishers)]
pub struct TrustedPublisher {
    pub id: i32,
    pub pubkey: String,
    pub display_name: String,
    pub tier: String,
    pub website: Option<String>,
    pub added_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
}

#[derive(Debug, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::plugin_trusted_publishers)]
pub struct NewTrustedPublisher {
    pub pubkey: String,
    pub display_name: String,
    pub tier: String,
    pub website: Option<String>,
    pub revoked_at: Option<NaiveDateTime>,
}

/// Single-row table holding the instance's local Ed25519 signing
/// keypair. `encrypted_sk` is AES-256-GCM ciphertext under the same
/// key material as MFA secrets (see `utils::encryption`).
#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::plugin_local_signing_key)]
pub struct LocalSigningKey {
    pub id: i32,
    pub pubkey: String,
    /// Framed AES-256-GCM blob (`utils::encryption::Keyring` shape).
    /// AAD = `b"nosdesk.plugin.local_sk.v1"` (singleton table; no row
    /// identity to bind beyond the domain tag).
    pub encrypted_sk: Vec<u8>,
    pub fingerprint: String,
    pub created_at: NaiveDateTime,
    pub encrypted_sk_kek_id: i16,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_local_signing_key)]
pub struct NewLocalSigningKey {
    pub id: i32,
    pub pubkey: String,
    pub encrypted_sk: Vec<u8>,
    pub encrypted_sk_kek_id: i16,
    pub fingerprint: String,
}

/// Single-row table that persists the anti-rollback counters from
/// the last registry snapshot the instance accepted. Durability
/// across restarts is load-bearing: without it, an attacker who
/// forces a restart could race the first boot fetch with an older
/// signed snapshot.
#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::plugin_registry_state)]
pub struct PluginRegistryState {
    pub id: i32,
    pub publishers_version: i64,
    pub index_version: i64,
    pub last_fetched_at: Option<NaiveDateTime>,
    pub last_fetch_error: Option<String>,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, AsChangeset, Default)]
#[diesel(table_name = crate::schema::plugin_registry_state)]
pub struct PluginRegistryStateUpdate {
    pub publishers_version: Option<i64>,
    pub index_version: Option<i64>,
    pub last_fetched_at: Option<Option<NaiveDateTime>>,
    pub last_fetch_error: Option<Option<String>>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Plugin data type - settings (admin-configured) or storage (plugin-managed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginDataType {
    Setting,
    Storage,
}

impl std::fmt::Display for PluginDataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginDataType::Setting => write!(f, "setting"),
            PluginDataType::Storage => write!(f, "storage"),
        }
    }
}

/// Consolidated plugin data (settings and storage in one table)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugin_data)]
pub struct PluginData {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub data_type: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    pub is_secret: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
}

impl PluginData {
    /// Check if this is a setting (admin-configured)
    pub fn is_setting(&self) -> bool {
        self.data_type == "setting"
    }

    /// Check if this is storage (plugin-managed)
    pub fn is_storage(&self) -> bool {
        self.data_type == "storage"
    }
}

/// New plugin data for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_data)]
pub struct NewPluginData {
    pub plugin_id: i32,
    pub data_type: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    pub is_secret: bool,
}

/// Plugin activity log entry
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugin_activity)]
pub struct PluginActivity {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub action: String,
    pub details: Option<serde_json::Value>,
    pub user_uuid: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
}

/// New plugin activity entry for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_activity)]
pub struct NewPluginActivity {
    pub plugin_id: i32,
    pub action: String,
    pub details: Option<serde_json::Value>,
    pub user_uuid: Option<Uuid>,
}
