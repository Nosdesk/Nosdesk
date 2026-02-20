use crate::db::DbConnection;
use crate::repository::site_settings;
use crate::utils::email::EmailBranding;

/// Get email branding from site settings, with fallbacks
pub fn get_email_branding(conn: &mut DbConnection, base_url: &str) -> EmailBranding {
    site_settings::get_site_settings(conn)
        .map(|settings| EmailBranding::new(
            settings.app_name,
            settings.logo_url,
            settings.primary_color,
            base_url.to_string(),
        ))
        .unwrap_or_else(|_| EmailBranding {
            app_name: "Nosdesk".to_string(),
            logo_url: None,
            primary_color: "#2563eb".to_string(),
            base_url: base_url.to_string(),
        })
}
