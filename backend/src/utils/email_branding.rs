use crate::db::DbConnection;
use crate::models::SiteSettings;
use crate::repository::site_settings;
use crate::utils::email::EmailBranding;

/// Get email branding from site settings, with fallbacks. Also
/// resolves the opt-in anti-phishing footer note so the email
/// template layer only has to render a ready string (or nothing).
pub fn get_email_branding(conn: &mut DbConnection, base_url: &str) -> EmailBranding {
    site_settings::get_site_settings(conn)
        .map(|settings| {
            let security_note = resolve_security_note(&settings, base_url);
            let mut branding = EmailBranding::new(
                settings.app_name,
                settings.logo_url,
                settings.primary_color,
                base_url.to_string(),
            );
            branding.security_note = security_note;
            branding
        })
        .unwrap_or_else(|_| EmailBranding {
            app_name: "Nosdesk".to_string(),
            logo_url: None,
            primary_color: "#2563eb".to_string(),
            base_url: base_url.to_string(),
            security_note: None,
        })
}

/// Resolve the anti-phishing footer note into a ready-to-render
/// string, or `None` when the workspace has it turned off.
///
/// Custom admin templates use `{{brand_name}}` / `{{domain}}` (the
/// same `{{var}}` shape as auto-ack); the built-in default is the
/// localized `email-security-note-default` FTL value. `{{domain}}`
/// resolves to the address the workspace sends mail from, since
/// that's what the recipient verifies against.
fn resolve_security_note(settings: &SiteSettings, base_url: &str) -> Option<String> {
    if !settings.email_security_note_enabled {
        return None;
    }

    let domain = outbound_email_domain().unwrap_or_else(|| host_from_url(base_url));

    let note = match settings.email_security_note_template.as_deref() {
        Some(custom) if !custom.trim().is_empty() => crate::utils::template_variables::substitute(
            custom,
            &[
                ("brand_name", settings.app_name.as_str()),
                ("domain", domain.as_str()),
            ],
        ),
        _ => {
            // The note is fixed boilerplate, so the workspace default
            // locale is good enough; we don't thread the recipient
            // locale through branding for a single footer line.
            let locale = crate::utils::locale::effective_locale(None, &settings.default_locale);
            crate::utils::i18n::tr_with(
                &locale,
                "email-security-note-default",
                &[
                    ("brand_name", settings.app_name.clone().into()),
                    ("domain", domain.clone().into()),
                ],
            )
        }
    };

    Some(note)
}

/// The domain the workspace sends mail from, taken from the outbound
/// "from" address. Mirrors `SMTP_FROM_EMAIL` / `RESEND_FROM_EMAIL`
/// resolution in `EmailConfig::from_env`. `None` when neither is set,
/// so the caller can fall back to the app host.
fn outbound_email_domain() -> std::option::Option<String> {
    std::env::var("SMTP_FROM_EMAIL")
        .or_else(|_| std::env::var("RESEND_FROM_EMAIL"))
        .ok()
        .and_then(|addr| addr.rsplit_once('@').map(|(_, d)| d.trim().to_string()))
        .filter(|d| !d.is_empty())
}

/// Best-effort host extraction from a base URL, used only as the
/// fallback for `{{domain}}` when no outbound from-address is set.
/// `https://desk.acme.com/app` -> `desk.acme.com`.
fn host_from_url(url: &str) -> String {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    without_scheme
        .split(['/', ':', '?', '#'])
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::host_from_url;

    #[test]
    fn host_from_url_strips_scheme_port_and_path() {
        assert_eq!(host_from_url("https://desk.acme.com/app"), "desk.acme.com");
        assert_eq!(host_from_url("http://localhost:3000"), "localhost");
        assert_eq!(host_from_url("desk.acme.com"), "desk.acme.com");
    }
}
