use diesel::prelude::*;
use uuid::Uuid;
use crate::db::DbConnection;
use crate::models::{SiteSettings, UpdateSiteSettings};
use crate::schema::site_settings;

/// Get site settings (always returns the single row, id=1)
pub fn get_site_settings(conn: &mut DbConnection) -> QueryResult<SiteSettings> {
    site_settings::table.find(1).first(conn)
}

/// Update site settings
pub fn update_site_settings(
    conn: &mut DbConnection,
    update: UpdateSiteSettings,
) -> QueryResult<SiteSettings> {
    diesel::update(site_settings::table.find(1))
        .set(&update)
        .get_result(conn)
}

/// Update logo URL
pub fn update_logo_url(
    conn: &mut DbConnection,
    logo_url: Option<String>,
    updated_by: Uuid,
) -> QueryResult<SiteSettings> {
    diesel::update(site_settings::table.find(1))
        .set((
            site_settings::logo_url.eq(logo_url),
            site_settings::updated_by.eq(Some(updated_by)),
        ))
        .get_result(conn)
}

/// Update light theme logo URL
pub fn update_logo_light_url(
    conn: &mut DbConnection,
    logo_light_url: Option<String>,
    updated_by: Uuid,
) -> QueryResult<SiteSettings> {
    diesel::update(site_settings::table.find(1))
        .set((
            site_settings::logo_light_url.eq(logo_light_url),
            site_settings::updated_by.eq(Some(updated_by)),
        ))
        .get_result(conn)
}

/// Update favicon URL
pub fn update_favicon_url(
    conn: &mut DbConnection,
    favicon_url: Option<String>,
    updated_by: Uuid,
) -> QueryResult<SiteSettings> {
    diesel::update(site_settings::table.find(1))
        .set((
            site_settings::favicon_url.eq(favicon_url),
            site_settings::updated_by.eq(Some(updated_by)),
        ))
        .get_result(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;
    use crate::models::UpdateSiteSettings;

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
        };

        let updated = update_site_settings(&mut conn, update).unwrap();
        assert_eq!(updated.app_name, "TestApp");
    }
}
