use crate::db::DbConnection;
use crate::repository::site_settings;
use crate::utils::email::EmailBranding;

/// Get email branding from site settings, with fallbacks
pub fn get_email_branding(conn: &mut DbConnection, base_url: &str) -> EmailBranding {
    match site_settings::get_site_settings(conn) {
        Ok(settings) => EmailBranding::new(
            settings.app_name,
            settings.logo_url,
            settings.primary_color,
            base_url.to_string(),
        ),
        Err(_) => {
            // Return defaults if settings can't be retrieved
            EmailBranding {
                app_name: "Nosdesk".to_string(),
                logo_url: None,
                primary_color: "#2563eb".to_string(),
                base_url: base_url.to_string(),
            }
        }
    }
}
