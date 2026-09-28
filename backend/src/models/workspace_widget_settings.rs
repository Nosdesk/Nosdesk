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
    /// Visitors without a signed identity get the help centre and guest form.
    pub allow_anonymous: bool,
    pub encrypted_secret: Option<Vec<u8>>,
    pub encrypted_kek_id: Option<i16>,
    pub workspace_id: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkspaceWidgetSettings {
    pub fn origins(&self) -> Vec<String> {
        self.allowed_origins.iter().flatten().cloned().collect()
    }
}
