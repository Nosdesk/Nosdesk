use chrono::{DateTime, Utc};
use diesel::prelude::*;

use crate::schema::workspace_widget_settings;

/// A workspace's embeddable widget settings. The signing secret never leaves
/// the repository in plaintext except to verify a visitor token (and once, to
/// the admin who generates it).
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = workspace_widget_settings)]
pub struct WorkspaceWidgetSettings {
    pub id: i32,
    pub enabled: bool,
    /// Sites allowed to embed the widget (`https://help.acme.com`,
    /// `https://*.acme.com`).
    pub allowed_origins: Vec<Option<String>>,
    pub encrypted_secret: Option<Vec<u8>>,
    pub encrypted_kek_id: Option<i16>,
    pub workspace_id: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// The secret before the last rotation, still accepted until
    /// `previous_valid_until` so a site can switch over.
    pub encrypted_previous_secret: Option<Vec<u8>>,
    pub previous_kek_id: Option<i16>,
    pub previous_valid_until: Option<DateTime<Utc>>,
}

impl WorkspaceWidgetSettings {
    pub fn origins(&self) -> Vec<String> {
        self.allowed_origins.iter().flatten().cloned().collect()
    }
}
