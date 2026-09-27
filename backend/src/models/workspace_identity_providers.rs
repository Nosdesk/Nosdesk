use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;

use crate::schema::workspace_identity_providers;

/// A workspace's OpenID Connect provider for requester sign-in. The client
/// secret never leaves the repository in plaintext except to the OIDC client.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = workspace_identity_providers)]
pub struct WorkspaceIdentityProvider {
    pub id: i32,
    /// `entra`, `google` or `oidc`.
    pub kind: String,
    pub display_name: String,
    pub issuer_url: String,
    pub client_id: String,
    pub encrypted_client_secret: Option<Vec<u8>>,
    pub encrypted_kek_id: Option<i16>,
    pub allowed_domains: Vec<Option<String>>,
    pub enabled: bool,
    pub workspace_id: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkspaceIdentityProvider {
    /// Lowercased allowed email domains.
    pub fn domains(&self) -> Vec<String> {
        self.allowed_domains
            .iter()
            .flatten()
            .map(|d| d.trim().to_lowercase())
            .filter(|d| !d.is_empty())
            .collect()
    }
}

/// What the admin page shows (no secret, only whether one is set).
#[derive(Debug, Serialize)]
pub struct IdentityProviderView {
    pub kind: String,
    pub display_name: String,
    pub issuer_url: String,
    pub client_id: String,
    pub has_client_secret: bool,
    pub allowed_domains: Vec<String>,
    pub enabled: bool,
}

impl From<&WorkspaceIdentityProvider> for IdentityProviderView {
    fn from(p: &WorkspaceIdentityProvider) -> Self {
        Self {
            kind: p.kind.clone(),
            display_name: p.display_name.clone(),
            issuer_url: p.issuer_url.clone(),
            client_id: p.client_id.clone(),
            has_client_secret: p.encrypted_client_secret.is_some(),
            allowed_domains: p.domains(),
            enabled: p.enabled,
        }
    }
}
