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
// sync-audit-only: Workspace settings; covered by the audit_log trigger on site_settings, sync clients don't subscribe
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
    if let Some(settings) = find_site_settings(conn)? {
        return Ok(settings);
    }
    ensure_row(conn)?;
    site_settings::table.first(conn)
}

/// The current workspace's site settings, if it has a row, without creating
/// one. For reads that may run with no workspace resolved (the public
/// branding route on an origin that names none), where an insert would fail
/// row-level security.
pub fn find_site_settings(conn: &mut DbConnection) -> QueryResult<Option<SiteSettings>> {
    site_settings::table.first::<SiteSettings>(conn).optional()
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

/// The name a workspace's `site_settings.app_name` starts with (the column
/// default). A row still holding it has never been named.
pub const DEFAULT_APP_NAME: &str = "Nosdesk";

// sync-audit-only: Workspace settings; covered by the audit_log trigger on site_settings, sync clients don't subscribe
/// Name the pinned workspace's portal and emails after the workspace, unless
/// an admin has already named them: creates the row with the workspace's name,
/// or replaces `app_name` only while it is still the default. Returns whether
/// it wrote. A no-op on an unpinned connection.
///
/// Hosted only; the caller checks. Must run pinned to the workspace (as the app
/// role, or under bypass after `pin_workspace`) so the audit trigger attributes
/// the write to it.
pub fn name_after_workspace(conn: &mut DbConnection) -> QueryResult<bool> {
    let written = diesel::sql_query(
        "INSERT INTO site_settings (app_name) \
         SELECT w.name FROM workspaces w \
         WHERE w.id = NULLIF(current_setting('app.workspace_id', true), '')::int \
         ON CONFLICT (workspace_id) DO UPDATE SET app_name = EXCLUDED.app_name \
         WHERE site_settings.app_name = $1",
    )
    .bind::<diesel::sql_types::Text, _>(DEFAULT_APP_NAME)
    .execute(conn)?;
    Ok(written > 0)
}

/// Every workspace whose portal and emails are still unnamed: no settings row,
/// or `app_name` still the default. Reads across workspaces, so the connection
/// must be elevated.
pub fn workspaces_with_default_app_name(conn: &mut DbConnection) -> QueryResult<Vec<i32>> {
    use crate::schema::workspaces;
    workspaces::table
        .left_join(site_settings::table)
        .filter(
            site_settings::id
                .is_null()
                .or(site_settings::app_name.eq(DEFAULT_APP_NAME)),
        )
        .select(workspaces::id)
        .order(workspaces::id)
        .load(conn)
}

// sync-audit-only: Workspace settings; covered by the audit_log trigger on site_settings, sync clients don't subscribe
/// Ahead of renaming a workspace, carry `app_name` to `new_name` when it still
/// equals the workspace's current name (nobody named it anything else). Call
/// in the rename's transaction, before the rename. Pins `workspace_id` for the
/// rest of the transaction so the audit row lands in that workspace; the
/// explicit filter keeps an elevated connection to that one row.
pub fn follow_workspace_rename(
    conn: &mut DbConnection,
    workspace_id: i32,
    new_name: &str,
) -> QueryResult<bool> {
    crate::sync::session::pin_workspace(conn, workspace_id)?;
    let updated = diesel::sql_query(
        "UPDATE site_settings SET app_name = $1 \
         FROM workspaces w \
         WHERE w.id = site_settings.workspace_id \
           AND site_settings.workspace_id = $2 \
           AND site_settings.app_name = w.name",
    )
    .bind::<diesel::sql_types::Text, _>(new_name)
    .bind::<diesel::sql_types::Integer, _>(workspace_id)
    .execute(conn)?;
    Ok(updated > 0)
}

/// One of the workspace's two logos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Logo {
    /// `logo_url`: the dark-theme logo, and the fallback for the light theme.
    Main,
    /// `logo_light_url`: the light-theme logo.
    Light,
}

// sync-audit-only: Workspace settings — covered by the audit_log trigger on site_settings; sync clients don't subscribe
/// Point a logo at a new upload, or clear it, together with its email copy,
/// so the two never disagree.
pub fn set_logo(
    conn: &mut DbConnection,
    logo: Logo,
    url: Option<String>,
    email_copy: Option<serde_json::Value>,
    updated_by: Uuid,
) -> QueryResult<SiteSettings> {
    ensure_row(conn)?;
    let update = diesel::update(site_settings::table);
    match logo {
        Logo::Main => update
            .set((
                site_settings::logo_url.eq(url),
                site_settings::email_logo.eq(email_copy),
                site_settings::updated_by.eq(Some(updated_by)),
            ))
            .get_result(conn),
        Logo::Light => update
            .set((
                site_settings::logo_light_url.eq(url),
                site_settings::email_logo_light.eq(email_copy),
                site_settings::updated_by.eq(Some(updated_by)),
            ))
            .get_result(conn),
    }
}

/// A workspace's logos that have no email copy. A URL is `None` when that
/// logo is unset or already has its copy.
#[derive(Debug, Queryable)]
pub struct LogosWithoutEmailCopy {
    pub workspace_id: i32,
    pub workspace_uuid: Uuid,
    pub logo_url: Option<String>,
    pub logo_light_url: Option<String>,
}

/// Every workspace with a logo that has no email copy. Reads across
/// workspaces, so the connection must be elevated.
pub fn logos_without_email_copies(
    conn: &mut DbConnection,
) -> QueryResult<Vec<LogosWithoutEmailCopy>> {
    use crate::schema::workspaces;
    use diesel::dsl::sql;
    use diesel::sql_types::{Nullable, Text};

    site_settings::table
        .inner_join(workspaces::table)
        .filter(
            site_settings::logo_url
                .is_not_null()
                .and(site_settings::email_logo.is_null())
                .or(site_settings::logo_light_url
                    .is_not_null()
                    .and(site_settings::email_logo_light.is_null())),
        )
        .select((
            site_settings::workspace_id,
            workspaces::uuid,
            sql::<Nullable<Text>>(
                "CASE WHEN site_settings.email_logo IS NULL THEN site_settings.logo_url END",
            ),
            sql::<Nullable<Text>>(
                "CASE WHEN site_settings.email_logo_light IS NULL \
                 THEN site_settings.logo_light_url END",
            ),
        ))
        .load(conn)
}

// sync-audit-only: Workspace settings — covered by the audit_log trigger on site_settings; sync clients don't subscribe
/// Record the email copy made for a logo after its upload, but only while the
/// logo is still `source_url` and has no copy: an upload since then made its
/// own. Returns whether it was recorded.
pub fn record_email_copy(
    conn: &mut DbConnection,
    logo: Logo,
    source_url: &str,
    email_copy: serde_json::Value,
) -> QueryResult<bool> {
    let updated = match logo {
        Logo::Main => diesel::update(
            site_settings::table
                .filter(site_settings::logo_url.eq(source_url))
                .filter(site_settings::email_logo.is_null()),
        )
        .set(site_settings::email_logo.eq(email_copy))
        .execute(conn)?,
        Logo::Light => diesel::update(
            site_settings::table
                .filter(site_settings::logo_light_url.eq(source_url))
                .filter(site_settings::email_logo_light.is_null()),
        )
        .set(site_settings::email_logo_light.eq(email_copy))
        .execute(conn)?,
    };
    Ok(updated > 0)
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
