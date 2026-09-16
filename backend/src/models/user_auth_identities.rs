use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Models for user authentication identities
#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Clone)]
#[diesel(table_name = crate::schema::user_auth_identities)]
pub struct UserAuthIdentity {
    pub id: i32,
    pub user_uuid: Uuid,
    pub provider_type: String,
    pub external_id: String,
    pub email: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub password_hash: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    pub created_by: Option<Uuid>,
    /// NULL = global login identity (local/microsoft/oidc); set = directory
    /// identity (ldap/scim) scoped to that workspace.
    pub workspace_id: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_auth_identities)]
pub struct NewUserAuthIdentity {
    pub user_uuid: Uuid,
    pub provider_type: String,
    pub external_id: String,
    pub email: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub password_hash: Option<String>,
    /// NULL = global login identity (local/microsoft/oidc); set = directory
    /// identity (ldap/scim) scoped to that workspace.
    pub workspace_id: Option<i32>,
}

// For displaying auth identities in the user profile
#[derive(Debug, Serialize, Deserialize)]
pub struct UserAuthIdentityDisplay {
    pub id: i32,
    pub provider_type: String,
    pub provider_name: String,
    pub email: Option<String>,
    pub created_at: NaiveDateTime,
}
