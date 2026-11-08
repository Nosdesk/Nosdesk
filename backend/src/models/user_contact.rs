use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---- Multi-valued contact: phones + addresses (workspace-scoped) -----------

fn default_phone_type() -> String {
    "work".to_string()
}
fn default_address_type() -> String {
    "work".to_string()
}

#[derive(Debug, Serialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::user_phone_numbers)]
pub struct UserPhoneNumber {
    pub id: i32,
    pub user_uuid: Uuid,
    pub workspace_id: i32,
    pub phone: String,
    pub phone_type: String,
    pub is_primary: bool,
    /// NULL = manual, a provider name (e.g. "microsoft") = sync-owned/read-only.
    pub source: Option<String>,
    pub label: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::user_phone_numbers)]
pub struct NewUserPhoneNumber {
    pub user_uuid: Uuid,
    pub phone: String,
    pub phone_type: String,
    pub is_primary: bool,
    pub source: Option<String>,
    pub label: Option<String>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct UserPhoneInput {
    pub phone: String,
    #[serde(default = "default_phone_type")]
    pub phone_type: String,
    #[serde(default)]
    pub is_primary: bool,
    pub label: Option<String>,
}

#[derive(Debug, Serialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::user_addresses)]
pub struct UserAddress {
    pub id: i32,
    pub user_uuid: Uuid,
    pub workspace_id: i32,
    pub address_type: String,
    pub is_primary: bool,
    pub street: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub source: Option<String>,
    pub label: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by: Option<Uuid>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// `house`, `street` or `locality`; `None` for a pin placed by hand.
    pub geocode_precision: Option<String>,
    /// The dataset the coordinates came from, or `manual`.
    pub geocode_source: Option<String>,
    pub geocode_place_confirmed: Option<bool>,
    pub geocoded_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Hash of the address text when it was last geocoded. Server-side only.
    #[serde(skip)]
    pub geocoded_hash: Option<String>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::user_addresses)]
pub struct NewUserAddress {
    pub user_uuid: Uuid,
    pub address_type: String,
    pub is_primary: bool,
    pub street: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub source: Option<String>,
    pub label: Option<String>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct UserAddressInput {
    #[serde(default = "default_address_type")]
    pub address_type: String,
    #[serde(default)]
    pub is_primary: bool,
    pub street: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub label: Option<String>,
}

/// Contact fields imported from a directory sync (Entra/Graph), mapped into a
/// user's profile (standard cols + `office_location`) and source='microsoft'
/// phone/address rows. Plain struct — no Graph-layer dependency in the repo.
#[derive(Debug, Default)]
pub struct DirectoryContact {
    pub job_title: Option<String>,
    pub organization: Option<String>,
    pub department: Option<String>,
    pub office_location: Option<String>,
    /// (number, phone_type)
    pub phones: Vec<(String, String)>,
    pub address: Option<DirectoryAddress>,
}

#[derive(Debug)]
pub struct DirectoryAddress {
    pub street: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
}
