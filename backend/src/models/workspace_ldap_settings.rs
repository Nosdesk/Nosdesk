use diesel::prelude::*;
use serde::{Deserialize, Serialize};

/// A workspace's LDAP/directory configuration (one row per workspace). The bind
/// password is stored KEK-encrypted; the encrypted columns are `serde(skip)` so
/// the secret can never leave the process in a serialized response (the handler
/// also returns a redacted DTO).
#[derive(Debug, Serialize, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::schema::workspace_ldap_settings)]
#[diesel(primary_key(workspace_id))]
pub struct WorkspaceLdapSettings {
    pub workspace_id: i32,
    pub enabled: bool,
    pub host: String,
    pub port: i32,
    pub tls_mode: String,
    pub verify_certs: bool,
    pub ca_cert_pem: Option<String>,
    pub follow_referrals: bool,
    pub connect_timeout_secs: i32,
    pub auth_mode: String,
    pub bind_dn: String,
    #[serde(skip)]
    pub encrypted_bind_password: Option<Vec<u8>>,
    #[serde(skip)]
    pub encrypted_kek_id: Option<i16>,
    pub user_base_dn: String,
    pub username_attribute: String,
    pub user_filter: String,
    pub page_size: i32,
    pub attribute_map: serde_json::Value,
    pub group_config: serde_json::Value,
    pub provisioning: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Editable LDAP settings (the admin PUT body + the upsert payload). Omits the
/// encrypted bind password (managed separately) and workspace_id (filled from
/// the `app.workspace_id` GUC default).
#[derive(Debug, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::workspace_ldap_settings)]
pub struct UpsertWorkspaceLdapSettings {
    pub enabled: bool,
    pub host: String,
    pub port: i32,
    pub tls_mode: String,
    pub verify_certs: bool,
    pub ca_cert_pem: Option<String>,
    pub follow_referrals: bool,
    pub connect_timeout_secs: i32,
    pub auth_mode: String,
    pub bind_dn: String,
    pub user_base_dn: String,
    pub username_attribute: String,
    pub user_filter: String,
    pub page_size: i32,
    pub attribute_map: serde_json::Value,
    pub group_config: serde_json::Value,
    pub provisioning: serde_json::Value,
}

/// A workspace's DirSync cursor state. The opaque cookie is client-held and must
/// survive restarts; NULL means no cursor yet (next run is a full sync).
#[derive(Debug, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::schema::workspace_ldap_sync_state)]
#[diesel(primary_key(workspace_id))]
pub struct WorkspaceLdapSyncState {
    pub workspace_id: i32,
    pub mechanism: String,
    pub cookie: Option<Vec<u8>>,
    pub last_full_reconcile_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
