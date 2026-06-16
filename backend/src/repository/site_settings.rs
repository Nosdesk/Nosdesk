use crate::db::DbConnection;
use crate::models::{SiteSettings, UpdateSiteSettings};
use crate::schema::site_settings;
use diesel::prelude::*;
use uuid::Uuid;

/// Ensure the current workspace has a `site_settings` row, lazily creating
/// a default one (every column from its DB default; `workspace_id` and the
/// sequence-backed `id` resolve automatically) if absent. Idempotent via
/// `ON CONFLICT (workspace_id)`.
///
/// site_settings is RLS-isolated by `workspace_id`, so the caller MUST be
/// workspace-scoped (`app.workspace_id` set, via `TenantConn` /
/// `with_actor_context`) — that GUC both scopes the read and fills the
/// `workspace_id` default on insert. Every settings access path is scoped.
pub(crate) fn ensure_row(conn: &mut DbConnection) -> QueryResult<()> {
    diesel::sql_query(
        "INSERT INTO site_settings DEFAULT VALUES ON CONFLICT (workspace_id) DO NOTHING",
    )
    .execute(conn)?;
    Ok(())
}

/// Get the current workspace's site settings, creating the default row on
/// first access. Returns exactly one row: RLS scopes the read to the
/// request's workspace (no hardcoded id), so this no longer collapses every
/// workspace onto a single global row.
pub fn get_site_settings(conn: &mut DbConnection) -> QueryResult<SiteSettings> {
    if let Some(settings) = site_settings::table
        .first::<SiteSettings>(conn)
        .optional()?
    {
        return Ok(settings);
    }
    ensure_row(conn)?;
    site_settings::table.first(conn)
}

// sync-audit-only: Workspace settings — covered by the audit_log trigger on site_settings; sync clients don't subscribe
/// Update the current workspace's site settings. The update targets the
/// RLS-visible row (the request's workspace); no `id` filter.
pub fn update_site_settings(
    conn: &mut DbConnection,
    update: UpdateSiteSettings,
) -> QueryResult<SiteSettings> {
    ensure_row(conn)?;
    diesel::update(site_settings::table)
        .set(&update)
        .get_result(conn)
}

// sync-audit-only: Workspace settings — covered by the audit_log trigger on site_settings; sync clients don't subscribe
/// Update logo URL
pub fn update_logo_url(
    conn: &mut DbConnection,
    logo_url: Option<String>,
    updated_by: Uuid,
) -> QueryResult<SiteSettings> {
    ensure_row(conn)?;
    diesel::update(site_settings::table)
        .set((
            site_settings::logo_url.eq(logo_url),
            site_settings::updated_by.eq(Some(updated_by)),
        ))
        .get_result(conn)
}

// sync-audit-only: Workspace settings — covered by the audit_log trigger on site_settings; sync clients don't subscribe
/// Update light theme logo URL
pub fn update_logo_light_url(
    conn: &mut DbConnection,
    logo_light_url: Option<String>,
    updated_by: Uuid,
) -> QueryResult<SiteSettings> {
    ensure_row(conn)?;
    diesel::update(site_settings::table)
        .set((
            site_settings::logo_light_url.eq(logo_light_url),
            site_settings::updated_by.eq(Some(updated_by)),
        ))
        .get_result(conn)
}

// sync-audit-only: Workspace settings — covered by the audit_log trigger on site_settings; sync clients don't subscribe
/// Update favicon URL
pub fn update_favicon_url(
    conn: &mut DbConnection,
    favicon_url: Option<String>,
    updated_by: Uuid,
) -> QueryResult<SiteSettings> {
    ensure_row(conn)?;
    diesel::update(site_settings::table)
        .set((
            site_settings::favicon_url.eq(favicon_url),
            site_settings::updated_by.eq(Some(updated_by)),
        ))
        .get_result(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UpdateSiteSettings;
    use crate::test_helpers::setup_test_connection;

    #[test]
    fn get_site_settings_returns_row() {
        let mut conn = setup_test_connection();
        let settings = get_site_settings(&mut conn);
        assert!(settings.is_ok());
    }

    #[test]
    fn update_site_settings_test() {
        let mut conn = setup_test_connection();
        let update = UpdateSiteSettings {
            app_name: Some("TestApp".to_string()),
            logo_url: None,
            logo_light_url: None,
            favicon_url: None,
            primary_color: None,
            updated_by: None,
            signature_default: None,
            ..Default::default()
        };

        let updated = update_site_settings(&mut conn, update).unwrap();
        assert_eq!(updated.app_name, "TestApp");
    }
}
