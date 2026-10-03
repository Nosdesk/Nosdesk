use tracing::warn;

use crate::db::DbConnection;
use crate::models::SiteSettings;
use crate::repository::site_settings;
use crate::services::outbound_email::OutboundEmailResolver;
use crate::utils::email::EmailBranding;
use crate::utils::email_logo::EmailLogo;

/// The identity a message goes out under. The security note names its
/// domain, since that is what a recipient checks the message against.
#[derive(Debug, Clone, Copy)]
pub enum SentFrom<'a> {
    /// The instance's own identity: account mail (password resets,
    /// invitations).
    Instance,
    /// The workspace's own identity: everything else it sends, replies and
    /// notifications included.
    Workspace,
    /// An address already chosen for the message (the test email).
    Address(&'a str),
}

/// Get email branding from site settings, with fallbacks. Also
/// resolves the opt-in anti-phishing footer note so the email
/// template layer only has to render a ready string (or nothing).
pub fn get_email_branding(
    conn: &mut DbConnection,
    base_url: &str,
    sent_from: SentFrom<'_>,
) -> EmailBranding {
    site_settings::get_site_settings(conn)
        .map(|settings| {
            let security_note = security_note(conn, &settings, base_url, sent_from);
            let logo = email_copy(settings.email_logo.as_ref());
            let logo_light = email_copy(settings.email_logo_light.as_ref());
            let mut branding = EmailBranding::new(
                settings.app_name,
                settings.primary_color,
                base_url.to_string(),
            );
            branding.logo = logo;
            branding.logo_light = logo_light;
            branding.security_note = security_note;
            branding
        })
        .unwrap_or_else(|_| EmailBranding {
            base_url: base_url.to_string(),
            ..EmailBranding::default()
        })
}

/// A logo's stored email copy. One that doesn't parse is left out, and the
/// letterhead falls back as if there were none.
fn email_copy(value: Option<&serde_json::Value>) -> Option<EmailLogo> {
    match serde_json::from_value(value?.clone()) {
        Ok(copy) => Some(copy),
        Err(e) => {
            warn!(error = %e, "Unreadable logo email copy");
            None
        }
    }
}

/// Resolve the anti-phishing footer note into a ready-to-render
/// string, or `None` when the workspace has it turned off.
///
/// Custom admin templates use `{{brand_name}}` / `{{domain}}` (the
/// same `{{var}}` shape as auto-ack); the built-in default is the
/// localized `email-security-note-default` FTL value. `{{domain}}` is
/// the domain the message leaves from (see [`SentFrom`]), else the
/// instance's From domain, else `base_url`'s host. With none of those
/// there is no domain to name, and no note.
pub fn security_note(
    conn: &mut DbConnection,
    settings: &SiteSettings,
    base_url: &str,
    sent_from: SentFrom<'_>,
) -> Option<String> {
    let resolver = crate::services::outbound_email::process_resolver();
    security_note_with(conn, settings, base_url, sent_from, resolver.as_deref())
}

fn security_note_with(
    conn: &mut DbConnection,
    settings: &SiteSettings,
    base_url: &str,
    sent_from: SentFrom<'_>,
    resolver: Option<&OutboundEmailResolver>,
) -> Option<String> {
    if !settings.email_security_note_enabled {
        return None;
    }

    let address = match sent_from {
        SentFrom::Instance => None,
        SentFrom::Address(address) => Some(address.to_string()),
        SentFrom::Workspace => resolver.and_then(|r| {
            r.workspace_from_address(conn, settings.workspace_id)
                .unwrap_or_else(|e| {
                    warn!(error = %e, "Could not read the workspace's sending address");
                    None
                })
        }),
    };
    let domain = address
        .as_deref()
        .and_then(domain_of)
        .or_else(outbound_email_domain)
        .unwrap_or_else(|| host_from_url(base_url));
    if domain.is_empty() {
        return None;
    }

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

/// The domain the instance sends its own mail from, taken from
/// `SMTP_FROM_EMAIL` as `EmailConfig::from_env` reads it. `None` when it is
/// unset.
pub(crate) fn outbound_email_domain() -> std::option::Option<String> {
    std::env::var("SMTP_FROM_EMAIL")
        .ok()
        .and_then(|addr| domain_of(&addr))
}

/// `support@acme.com` -> `acme.com`.
fn domain_of(address: &str) -> Option<String> {
    address
        .rsplit_once('@')
        .map(|(_, domain)| domain.trim().to_string())
        .filter(|domain| !domain.is_empty())
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
    use super::*;
    use crate::models::{UpdateSiteSettings, UpsertWorkspaceEmailSettings};
    use crate::test_helpers::{setup_test_connection, setup_test_pool};
    use crate::utils::email::{EmailConfig, EmailService, SmtpSecurity};
    use std::sync::Arc;

    fn note_on(conn: &mut DbConnection, template: Option<&str>) -> SiteSettings {
        site_settings::update_site_settings(
            conn,
            UpdateSiteSettings {
                app_name: Some("Acme IT".into()),
                email_security_note_enabled: Some(true),
                email_security_note_template: Some(template.map(str::to_string)),
                ..Default::default()
            },
        )
        .unwrap()
    }

    /// Hosted, with a tenant domain: a workspace with no identity of its own
    /// sends from `support@<slug>.nosdesk.test`.
    fn hosted_resolver() -> OutboundEmailResolver {
        let platform = EmailService::new(EmailConfig {
            smtp_host: "smtp.platform.test".into(),
            smtp_port: 587,
            smtp_username: String::new(),
            smtp_password: String::new(),
            from_name: "Platform".into(),
            from_email: "noreply@platform.test".into(),
            enabled: true,
            security: SmtpSecurity::StartTls,
        });
        OutboundEmailResolver::with_policy(
            setup_test_pool(),
            Some(Arc::new(platform)),
            false,
            Some("nosdesk.test".into()),
        )
    }

    const TEMPLATE: &str = "{{brand_name}} only emails you from {{domain}}.";

    #[test]
    fn no_note_while_it_is_off() {
        let mut conn = setup_test_connection();
        let settings = site_settings::get_site_settings(&mut conn).unwrap();
        let resolver = hosted_resolver();
        let note = security_note_with(
            &mut conn,
            &settings,
            "https://desk.example.com",
            SentFrom::Workspace,
            Some(&resolver),
        );
        assert_eq!(note, None);
    }

    #[test]
    fn workspace_mail_names_the_domain_the_workspace_sends_from() {
        let mut conn = setup_test_connection();
        let settings = note_on(&mut conn, Some(TEMPLATE));
        let resolver = hosted_resolver();
        let note = |conn: &mut DbConnection| {
            security_note_with(
                conn,
                &settings,
                "https://desk.example.com",
                SentFrom::Workspace,
                Some(&resolver),
            )
        };
        assert_eq!(
            note(&mut conn).as_deref(),
            Some("Acme IT only emails you from default.nosdesk.test."),
            "the managed address, not the platform's"
        );

        crate::repository::workspace_email_settings::upsert(
            &mut conn,
            UpsertWorkspaceEmailSettings {
                enabled: true,
                from_name: "Acme IT".into(),
                from_email: "help@acme.test".into(),
                smtp_host: "smtp.acme.test".into(),
                smtp_port: 465,
                smtp_security: "tls".into(),
                smtp_username: "acme".into(),
                sending_mode: "smtp_relay".into(),
            },
        )
        .unwrap();
        assert_eq!(
            note(&mut conn).as_deref(),
            Some("Acme IT only emails you from acme.test."),
            "its own relay"
        );
    }

    #[test]
    fn a_chosen_sender_is_named_as_is() {
        let mut conn = setup_test_connection();
        let settings = note_on(&mut conn, Some(TEMPLATE));
        let note = security_note_with(
            &mut conn,
            &settings,
            "https://desk.example.com",
            SentFrom::Address("Support@Relay.acme.test"),
            None,
        );
        assert_eq!(
            note.as_deref(),
            Some("Acme IT only emails you from Relay.acme.test.")
        );
    }

    #[test]
    fn account_mail_names_the_instance_domain() {
        let mut conn = setup_test_connection();
        let settings = note_on(&mut conn, Some(TEMPLATE));
        let resolver = hosted_resolver();
        let expected = outbound_email_domain().unwrap_or_else(|| "desk.example.com".into());
        let note = security_note_with(
            &mut conn,
            &settings,
            "https://desk.example.com/app",
            SentFrom::Instance,
            Some(&resolver),
        );
        assert_eq!(
            note,
            Some(format!("Acme IT only emails you from {expected}.")),
            "never the workspace's managed address"
        );
    }

    #[test]
    fn the_default_wording_carries_the_domain() {
        let mut conn = setup_test_connection();
        let settings = note_on(&mut conn, None);
        let resolver = hosted_resolver();
        let note = security_note_with(
            &mut conn,
            &settings,
            "https://desk.example.com",
            SentFrom::Workspace,
            Some(&resolver),
        )
        .expect("a note");
        assert!(note.contains("Acme IT"), "{note}");
        assert!(note.contains("default.nosdesk.test"), "{note}");
    }

    #[test]
    fn domain_of_takes_the_part_after_the_at() {
        assert_eq!(domain_of("help@acme.test").as_deref(), Some("acme.test"));
        assert_eq!(domain_of("no-at-sign"), None);
        assert_eq!(domain_of("trailing@"), None);
    }

    #[test]
    fn host_from_url_strips_scheme_port_and_path() {
        assert_eq!(host_from_url("https://desk.acme.com/app"), "desk.acme.com");
        assert_eq!(host_from_url("http://localhost:3000"), "localhost");
        assert_eq!(host_from_url("desk.acme.com"), "desk.acme.com");
    }
}
