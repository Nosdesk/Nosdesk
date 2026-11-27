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

/// The most of a workspace's name that mail to an unproven address carries.
pub const UNPROVEN_NAME_SLOT_CHARS: usize = 40;

/// Branding for mail the instance sends under the platform's own sender, on
/// a hosted deployment, to an address nobody has proven: the guest form's
/// confirmation, an invitation, a reset for an unproven address. Whoever typed
/// the address may not own it, and the mail leaves under the platform's name,
/// so the workspace gets one thing in it: its name, as plain text, cut to
/// [`UNPROVEN_NAME_SLOT_CHARS`]. No logo, no colour of its own and no security
/// note it wrote. The same for every hosted workspace. `conn` must be pinned
/// to the workspace.
pub fn unproven_recipient_branding(conn: &mut DbConnection, base_url: &str) -> EmailBranding {
    // The name the workspace gives its portal and email (`app_name`, which
    // hosted provisioning sets from the workspace's name); the bootstrap
    // workspace's own name is a placeholder.
    let name = site_settings::get_site_settings(conn)
        .map(|s| s.app_name)
        .unwrap_or_default();
    let name = name_slot(&name);
    EmailBranding {
        app_name: if name.is_empty() {
            site_settings::DEFAULT_APP_NAME.to_string()
        } else {
            name
        },
        base_url: base_url.to_string(),
        ..EmailBranding::default()
    }
}

/// How instance-identity mail to one address may speak for the workspace.
pub struct InstanceMail {
    pub branding: EmailBranding,
    /// Whether it may carry names a workspace or a form wrote (the account's
    /// name, who invited them). Pass them empty when not.
    pub names: bool,
}

/// [`InstanceMail`] for `recipient`. Self-hosted, the instance's sender is the
/// operator's own, so the workspace keeps its branding and its names. Hosted,
/// the sender is the platform's: an address no account has proven (a sign-in
/// code, a confirmed link or a confirmation POST, `user_emails.proven_at`)
/// gets [`unproven_recipient_branding`] and no names. A directory or a
/// workspace's own sign-in can mark an address verified; that isn't proof of
/// the mailbox, so `is_verified` doesn't count here.
pub fn instance_mail_for(conn: &mut DbConnection, base_url: &str, recipient: &str) -> InstanceMail {
    let proven = !crate::middleware::workspace_context::is_hosted()
        || crate::repository::user_emails::holders_of(conn, recipient)
            .map(|rows| rows.iter().any(|e| e.proven_at.is_some()))
            .unwrap_or(false);
    if proven {
        InstanceMail {
            branding: get_email_branding(conn, base_url, SentFrom::Instance),
            names: true,
        }
    } else {
        InstanceMail {
            branding: unproven_recipient_branding(conn, base_url),
            names: false,
        }
    }
}

/// Branding for the request form's confirmation, which always goes to an
/// address nobody has proven: [`unproven_recipient_branding`] on a hosted
/// deployment, the workspace's own self-hosted.
pub fn guest_confirmation_branding(conn: &mut DbConnection, base_url: &str) -> EmailBranding {
    if crate::middleware::workspace_context::is_hosted() {
        unproven_recipient_branding(conn, base_url)
    } else {
        get_email_branding(conn, base_url, SentFrom::Instance)
    }
}

/// `name` as one plain line of at most [`UNPROVEN_NAME_SLOT_CHARS`]
/// characters, counted as grapheme clusters so none is cut in half. Control
/// characters, line and paragraph separators become spaces; format characters
/// (bidirectional overrides and isolates, zero-width characters, the byte
/// order mark), which can reorder or hide what a reader sees, are dropped.
/// Runs of whitespace become one space.
pub fn name_slot(name: &str) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    let flat: String = name
        .chars()
        .filter(|c| !is_format_char(*c))
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let line = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    line.graphemes(true)
        .take(UNPROVEN_NAME_SLOT_CHARS)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Unicode format characters (general category Cf) that turn up in text:
/// the soft hyphen, bidirectional marks, embeddings, overrides and isolates,
/// zero-width spaces and joiners, invisible operators, the byte order mark,
/// interlinear annotation marks and tag characters.
fn is_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
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

    #[test]
    fn the_name_slot_is_one_plain_line_of_forty_graphemes() {
        use super::name_slot;
        assert_eq!(
            name_slot("Acme\nSupport\u{2028}Desk\t "),
            "Acme Support Desk"
        );
        // A right-to-left override would show "moc.live" as "evil.com".
        assert_eq!(name_slot("Acme \u{202E}moc.live\u{202C}"), "Acme moc.live");
        assert_eq!(
            name_slot("Ac\u{200B}me\u{FEFF} \u{2066}IT\u{2069}"),
            "Acme IT"
        );
        // Forty clusters, none cut: "e" with a combining acute is one.
        let accented = "e\u{0301}".repeat(45);
        let slot = name_slot(&accented);
        assert_eq!(slot, "e\u{0301}".repeat(40));
        let flags = "\u{1F1E6}\u{1F1FA}".repeat(41);
        assert_eq!(name_slot(&flags), "\u{1F1E6}\u{1F1FA}".repeat(40));
        assert_eq!(name_slot(&"x".repeat(60)).chars().count(), 40);
    }
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
