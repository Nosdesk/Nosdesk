use async_trait::async_trait;
use lettre::{
    message::{
        header::{ContentType, Header, HeaderName, HeaderValue, InReplyTo, MessageId, References},
        Mailbox, MultiPart, SinglePart,
    },
    transport::smtp::authentication::Credentials,
    Message, SmtpTransport, Transport,
};
use std::sync::Arc;

/// `Auto-Submitted` header (RFC 3834 §5). Set on any system-authored
/// reply we send (currently just the auto-acknowledgement on new
/// tickets). An auto-responder on the customer's end should see this
/// value and silently drop the message rather than bouncing back an
/// OOO — which is exactly how we handle the same header on inbound
/// (see `email_imap::detect_loop_markers`). Without this header our
/// auto-ack can ping-pong forever against an Exchange OOO.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AutoSubmitted(String);
impl Header for AutoSubmitted {
    fn name() -> HeaderName {
        HeaderName::new_from_ascii_str("Auto-Submitted")
    }
    fn parse(s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self(s.into()))
    }
    fn display(&self) -> HeaderValue {
        HeaderValue::new(Self::name(), self.0.clone())
    }
}

/// `X-Auto-Response-Suppress` — Microsoft / Exchange-specific
/// loop-breaker. Honoured by Outlook and Exchange Online's
/// transport rules; harmless on other MTAs. Paired with
/// `Auto-Submitted` to cover both the RFC 3834 world and the
/// Exchange world.
#[derive(Debug, Clone, PartialEq, Eq)]
struct XAutoResponseSuppress(String);
impl Header for XAutoResponseSuppress {
    fn name() -> HeaderName {
        HeaderName::new_from_ascii_str("X-Auto-Response-Suppress")
    }
    fn parse(s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self(s.into()))
    }
    fn display(&self) -> HeaderValue {
        HeaderValue::new(Self::name(), self.0.clone())
    }
}
use std::env;
use std::str::FromStr;

/// Build the plaintext `SinglePart` used in outbound replies with
/// the `format=flowed` parameter declared (RFC 3676). Our generated
/// plaintext (`> ` quote prefixes, `-- ` signature separator,
/// hard-wrapped line breaks via `\n`) already follows the format-
/// flowed conventions, but without the parameter clients are free
/// to soft-wrap our lines and break the quote/signature alignment
/// on narrow viewports. `delsp=no` keeps trailing whitespace —
/// we never emit soft breaks (lines ending in a space) so this is
/// strictly correct.
fn plaintext_flowed_part(body: String) -> SinglePart {
    SinglePart::builder()
        .header(
            ContentType::parse("text/plain; charset=utf-8; format=flowed; delsp=no")
                .expect("valid content-type literal"),
        )
        .body(body)
}

/// Simple HTML escaping for email content to prevent XSS
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Branding configuration for email templates
#[derive(Debug, Clone)]
pub struct EmailBranding {
    pub app_name: String,
    pub logo_url: Option<String>,
    pub primary_color: String,
    pub base_url: String,
}

impl Default for EmailBranding {
    fn default() -> Self {
        Self {
            app_name: "Nosdesk".to_string(),
            logo_url: None,
            primary_color: "#2563eb".to_string(),
            base_url: env::var("FRONTEND_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_string()),
        }
    }
}

impl EmailBranding {
    /// Create branding config from site settings
    pub fn new(
        app_name: String,
        logo_url: Option<String>,
        primary_color: Option<String>,
        base_url: String,
    ) -> Self {
        Self {
            app_name,
            logo_url,
            primary_color: primary_color.unwrap_or_else(|| "#2563eb".to_string()),
            base_url,
        }
    }

    /// Generate lighter shade of primary color for backgrounds
    fn primary_color_light(&self) -> String {
        if let Some(hex) = self.primary_color.strip_prefix('#') {
            if hex.len() == 6 {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(&hex[0..2], 16),
                    u8::from_str_radix(&hex[2..4], 16),
                    u8::from_str_radix(&hex[4..6], 16),
                ) {
                    // Mix with white (very light tint)
                    let lighten = |c: u8| ((c as f32 * 0.15) + (255.0 * 0.85)) as u8;
                    return format!("#{:02x}{:02x}{:02x}", lighten(r), lighten(g), lighten(b));
                }
            }
        }
        "#eff6ff".to_string() // fallback
    }
}

/// Email template builder for consistent, branded emails
struct EmailTemplate<'a> {
    branding: &'a EmailBranding,
}

impl<'a> EmailTemplate<'a> {
    fn new(branding: &'a EmailBranding) -> Self {
        Self { branding }
    }

    /// Build complete HTML email with branding. `lang` lands on
    /// the `<html lang="...">` attribute so screen readers reading
    /// the rendered message announce the right pronunciation rules
    /// and clients that auto-translate inbound mail know to skip
    /// (the body already matches).
    #[allow(clippy::too_many_arguments)]
    fn build(
        &self,
        title: &str,
        title_color: &str,
        content: &str,
        button_text: &str,
        button_url: &str,
        button_color: &str,
        notice_type: NoticeType,
        notice_items: &[&str],
        footer_text: &str,
        locale: &unic_langid::LanguageIdentifier,
    ) -> String {
        let logo_html = self.build_logo_section();
        let notice_html = self.build_notice_section(notice_type, notice_items, locale);
        let lang = locale.to_string();
        let fallback_link_prompt = crate::utils::i18n::tr(locale, "email-link-fallback-prompt");
        let rights_reserved = crate::utils::i18n::tr(locale, "email-footer-rights");
        let automated_notice = crate::utils::i18n::tr(locale, "email-footer-automated");

        format!(
            r#"<!DOCTYPE html>
<html lang="{lang}">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <meta http-equiv="X-UA-Compatible" content="IE=edge">
    <title>{title}</title>
    <!--[if mso]>
    <noscript>
        <xml>
            <o:OfficeDocumentSettings>
                <o:PixelsPerInch>96</o:PixelsPerInch>
            </o:OfficeDocumentSettings>
        </xml>
    </noscript>
    <![endif]-->
</head>
<body style="margin: 0; padding: 0; background-color: #f3f4f6; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif; -webkit-font-smoothing: antialiased;">
    <!-- Preview text (hidden) -->
    <div style="display: none; max-height: 0; overflow: hidden;">
        {title} - {app_name}
    </div>

    <!-- Email wrapper -->
    <table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%" style="background-color: #f3f4f6;">
        <tr>
            <td style="padding: 40px 20px;">
                <!-- Main container -->
                <table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%" style="max-width: 600px; margin: 0 auto;">

                    <!-- Header with logo -->
                    <tr>
                        <td style="background-color: #ffffff; border-radius: 12px 12px 0 0; padding: 32px 40px; text-align: center; border-bottom: 1px solid #e5e7eb;">
                            {logo_html}
                        </td>
                    </tr>

                    <!-- Title bar -->
                    <tr>
                        <td style="background-color: {title_color}; padding: 24px 40px;">
                            <h1 style="margin: 0; color: #ffffff; font-size: 22px; font-weight: 600; text-align: center; letter-spacing: -0.02em;">
                                {title}
                            </h1>
                        </td>
                    </tr>

                    <!-- Content area -->
                    <tr>
                        <td style="background-color: #ffffff; padding: 40px;">
                            {content}

                            <!-- CTA Button -->
                            <table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%" style="margin: 32px 0;">
                                <tr>
                                    <td style="text-align: center;">
                                        <!--[if mso]>
                                        <v:roundrect xmlns:v="urn:schemas-microsoft-com:vml" xmlns:w="urn:schemas-microsoft-com:office:word" href="{button_url}" style="height:48px;v-text-anchor:middle;width:220px;" arcsize="12%" strokecolor="{button_color}" fillcolor="{button_color}">
                                        <w:anchorlock/>
                                        <center style="color:#ffffff;font-family:sans-serif;font-size:16px;font-weight:600;">{button_text}</center>
                                        </v:roundrect>
                                        <![endif]-->
                                        <!--[if !mso]><!-->
                                        <a href="{button_url}" target="_blank" style="display: inline-block; background-color: {button_color}; color: #ffffff; text-decoration: none; padding: 14px 32px; border-radius: 8px; font-weight: 600; font-size: 16px; transition: background-color 0.2s;">
                                            {button_text}
                                        </a>
                                        <!--<![endif]-->
                                    </td>
                                </tr>
                            </table>

                            <!-- Fallback link -->
                            <p style="margin: 0 0 24px 0; color: #6b7280; font-size: 13px; line-height: 1.5; text-align: center;">
                                {fallback_link_prompt}
                            </p>
                            <p style="margin: 0 0 32px 0; padding: 12px 16px; background-color: #f9fafb; border-radius: 6px; word-break: break-all; font-size: 12px; color: {primary_color}; font-family: 'SF Mono', Monaco, 'Courier New', monospace;">
                                <a href="{button_url}" style="color: {primary_color}; text-decoration: none;">{button_url}</a>
                            </p>

                            <!-- Notice box -->
                            {notice_html}
                        </td>
                    </tr>

                    <!-- Footer -->
                    <tr>
                        <td style="background-color: #f9fafb; border-radius: 0 0 12px 12px; padding: 24px 40px; border-top: 1px solid #e5e7eb;">
                            <p style="margin: 0 0 8px 0; color: #6b7280; font-size: 13px; line-height: 1.5; text-align: center;">
                                {footer_text}
                            </p>
                            <p style="margin: 0; color: #9ca3af; font-size: 12px; text-align: center;">
                                &copy; {year} {app_name}. {rights_reserved}
                            </p>
                        </td>
                    </tr>

                </table>

                <!-- Unsubscribe / Help links -->
                <table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%" style="max-width: 600px; margin: 24px auto 0;">
                    <tr>
                        <td style="text-align: center;">
                            <p style="margin: 0; color: #9ca3af; font-size: 12px;">
                                {automated_notice}
                            </p>
                        </td>
                    </tr>
                </table>

            </td>
        </tr>
    </table>
</body>
</html>"#,
            title = escape_html(title),
            app_name = escape_html(&self.branding.app_name),
            logo_html = logo_html,
            title_color = title_color,
            content = content,
            button_text = escape_html(button_text),
            button_url = button_url,
            button_color = button_color,
            notice_html = notice_html,
            footer_text = escape_html(footer_text),
            primary_color = &self.branding.primary_color,
            year = chrono::Utc::now().format("%Y"),
        )
    }

    /// Build the logo section HTML
    fn build_logo_section(&self) -> String {
        match &self.branding.logo_url {
            Some(logo_url) if !logo_url.is_empty() => {
                // Construct full URL if relative path
                let full_url = if logo_url.starts_with("http") {
                    logo_url.clone()
                } else {
                    format!("{}{}", self.branding.base_url, logo_url)
                };
                format!(
                    r#"<img src="{}" alt="{}" style="max-width: 180px; max-height: 60px; height: auto;" />"#,
                    escape_html(&full_url),
                    escape_html(&self.branding.app_name)
                )
            }
            _ => {
                // Text-based logo fallback with primary color
                format!(
                    r#"<h2 style="margin: 0; color: {}; font-size: 28px; font-weight: 700; letter-spacing: -0.03em;">{}</h2>"#,
                    &self.branding.primary_color,
                    escape_html(&self.branding.app_name)
                )
            }
        }
    }

    /// Build the notice section HTML
    fn build_notice_section(
        &self,
        notice_type: NoticeType,
        items: &[&str],
        locale: &unic_langid::LanguageIdentifier,
    ) -> String {
        if items.is_empty() {
            return String::new();
        }

        let light_color = self.branding.primary_color_light();

        let (bg_color, border_color): (&str, &str) = match notice_type {
            NoticeType::Warning => ("#fef3c7", "#f59e0b"),
            NoticeType::Critical => ("#fee2e2", "#dc2626"),
            NoticeType::Info => (&light_color, &self.branding.primary_color),
            NoticeType::Success => ("#ecfdf5", "#059669"),
        };

        let title_key = match notice_type {
            NoticeType::Warning => "email-notice-security",
            NoticeType::Critical => "email-notice-security-critical",
            NoticeType::Info => "email-notice-getting-started",
            NoticeType::Success => "email-notice-success",
        };
        let title = crate::utils::i18n::tr(locale, title_key);

        let items_html: String = items
            .iter()
            .map(|item| format!(
                r#"<li style="margin: 0 0 8px 0; color: #374151; font-size: 14px; line-height: 1.5;">{item}</li>"#
            ))
            .collect();

        format!(
            r#"<table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%" style="margin-top: 8px;">
                <tr>
                    <td style="background-color: {bg_color}; border-left: 4px solid {border_color}; border-radius: 0 8px 8px 0; padding: 20px;">
                        <p style="margin: 0 0 12px 0; font-weight: 600; color: #111827; font-size: 14px;">
                            {title}
                        </p>
                        <ul style="margin: 0; padding: 0 0 0 20px;">
                            {items_html}
                        </ul>
                    </td>
                </tr>
            </table>"#,
        )
    }
}

/// Notice type for email templates
#[derive(Clone, Copy)]
enum NoticeType {
    Warning,
    #[allow(dead_code)]
    Critical,
    Info,
    #[allow(dead_code)]
    Success,
}

/// Email configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct EmailConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub from_name: String,
    pub from_email: String,
    pub enabled: bool,
    /// Connection security. Defaults to [`SmtpSecurity::StartTls`] —
    /// the production path. [`SmtpSecurity::Plaintext`] exists for local
    /// integration tests against Greenmail (port 3025 is plaintext).
    /// NEVER set to `None` in production; credentials ride the wire
    /// in the clear.
    pub security: SmtpSecurity,
}

/// SMTP transport security mode. Kept as its own enum so calling code
/// can't accidentally set `smtp_port = 3025` and silently fall into a
/// plaintext send path — the two values are set explicitly together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpSecurity {
    /// Implicit TLS on port 465 (`relay()`).
    Tls,
    /// STARTTLS upgrade on port 587 (`starttls_relay()`). Default for
    /// all env-loaded configs.
    StartTls,
    /// No TLS. Intended only for local test servers (Greenmail, Mailpit).
    /// Named `Plaintext` rather than `None` so the variant is impossible
    /// to mistake for "I don't care"; production configs that land on
    /// this value are bugs.
    Plaintext,
}

impl EmailConfig {
    /// Load email configuration from environment variables
    pub fn from_env() -> Result<Self, String> {
        let enabled = env::var("SMTP_ENABLED")
            .unwrap_or_else(|_| "false".to_string())
            .parse::<bool>()
            .unwrap_or(false);

        // If disabled, return minimal config
        if !enabled {
            return Ok(Self {
                smtp_host: String::new(),
                smtp_port: 587,
                smtp_username: String::new(),
                smtp_password: String::new(),
                from_name: String::new(),
                from_email: String::new(),
                enabled: false,
                security: SmtpSecurity::StartTls,
            });
        }

        let smtp_host =
            env::var("SMTP_HOST").map_err(|_| "SMTP_HOST not configured".to_string())?;

        let smtp_port = env::var("SMTP_PORT")
            .unwrap_or_else(|_| "587".to_string())
            .parse::<u16>()
            .map_err(|_| "Invalid SMTP_PORT".to_string())?;

        let smtp_username =
            env::var("SMTP_USERNAME").map_err(|_| "SMTP_USERNAME not configured".to_string())?;

        let smtp_password =
            env::var("SMTP_PASSWORD").map_err(|_| "SMTP_PASSWORD not configured".to_string())?;

        let from_name = env::var("SMTP_FROM_NAME").unwrap_or_else(|_| "Nosdesk".to_string());

        let from_email = env::var("SMTP_FROM_EMAIL")
            .or_else(|_| env::var("SMTP_USERNAME"))
            .map_err(|_| "SMTP_FROM_EMAIL not configured".to_string())?;

        // Optional explicit security selector. Defaults to StartTLS
        // for backward compatibility — every legitimate production
        // SMTP relay supports it. `plaintext` exists for local
        // testing against Mailpit / Greenmail; never set this in
        // production, the doc on `SmtpSecurity::Plaintext` flags it
        // as a misconfiguration.
        let security = match env::var("SMTP_SECURITY")
            .ok()
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("tls") => SmtpSecurity::Tls,
            Some("plaintext" | "plain" | "none") => SmtpSecurity::Plaintext,
            Some("starttls") | None => SmtpSecurity::StartTls,
            Some(other) => {
                return Err(format!(
                    "Invalid SMTP_SECURITY '{other}'; expected tls | starttls | plaintext"
                ));
            }
        };

        Ok(Self {
            smtp_host,
            smtp_port,
            smtp_username,
            smtp_password,
            from_name,
            from_email,
            enabled,
            security,
        })
    }

    /// Get the from mailbox for emails
    pub fn from_mailbox(&self) -> Result<Mailbox, String> {
        format!("{} <{}>", self.from_name, self.from_email)
            .parse()
            .map_err(|e| format!("Invalid from address: {e}"))
    }

    /// Check if email is properly configured
    pub fn is_configured(&self) -> bool {
        self.enabled
            && !self.smtp_host.is_empty()
            && !self.smtp_username.is_empty()
            && !self.smtp_password.is_empty()
    }
}

/// Build a lettre SMTP mailer from config. Shared by the SMTP transport
/// and the legacy direct-send methods so the connection/security wiring
/// lives in one place.
fn build_smtp_mailer(config: &EmailConfig) -> Result<SmtpTransport, String> {
    let creds = Credentials::new(config.smtp_username.clone(), config.smtp_password.clone());

    let builder = match config.security {
        SmtpSecurity::Tls => SmtpTransport::relay(&config.smtp_host)
            .map_err(|e| format!("Failed to create SMTP transport: {e}"))?,
        SmtpSecurity::StartTls => SmtpTransport::starttls_relay(&config.smtp_host)
            .map_err(|e| format!("Failed to create SMTP transport: {e}"))?,
        // Plain transport for local test servers — no TLS, no auth
        // negotiation. Credentials still ride but the connection is
        // cleartext, so `SmtpSecurity::Plaintext`'s doc flags it.
        SmtpSecurity::Plaintext => SmtpTransport::builder_dangerous(&config.smtp_host),
    };

    Ok(builder.port(config.smtp_port).credentials(creds).build())
}

/// Build the lettre `Message` for an outbound queue message. Shared by
/// the SMTP transport and `EmailService::build_ticket_reply_message`
/// (the latter kept so unit tests can inspect the serialized form).
fn build_outbound_message(
    config: &EmailConfig,
    outbound: &OutboundEmailMessage<'_>,
) -> Result<Message, String> {
    let to_mailbox: Mailbox = outbound
        .to
        .parse()
        .map_err(|e| format!("Invalid recipient email: {e}"))?;

    let mut builder = Message::builder()
        .from(config.from_mailbox()?)
        .to(to_mailbox)
        .subject(outbound.subject)
        .header(MessageId::from(format!("<{}>", outbound.message_id)));

    if let Some(parent) = outbound.in_reply_to {
        builder = builder.header(InReplyTo::from(parent.to_string()));
    }
    if !outbound.references.is_empty() {
        builder = builder.header(References::from(outbound.references.join(" ")));
    }
    // RFC 3834 + Exchange loop-prevention headers for auto-replies.
    if outbound.auto_submitted {
        builder = builder
            .header(AutoSubmitted("auto-replied".to_string()))
            .header(XAutoResponseSuppress("All".to_string()));
    }

    // Prefer multipart/alternative when both text + html are given so
    // clients can pick; text-only falls back to a single part. Both
    // declare `format=flowed` so clients don't soft-wrap our `> ` quote
    // prefixes or `-- ` signature separator.
    let message = if let Some(html) = outbound.body_html {
        builder
            .multipart(
                MultiPart::alternative()
                    .singlepart(plaintext_flowed_part(outbound.body_text.to_string()))
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_HTML)
                            .body(html.to_string()),
                    ),
            )
            .map_err(|e| format!("Failed to build ticket reply: {e}"))?
    } else {
        builder
            .singlepart(plaintext_flowed_part(outbound.body_text.to_string()))
            .map_err(|e| format!("Failed to build ticket reply: {e}"))?
    };

    Ok(message)
}

/// Outcome of a transport send. Carries the provider message id when the
/// backend returns one (e.g. Resend's `email_id`); `None` for SMTP, where
/// the RFC Message-ID is the only identity.
#[allow(dead_code)]
pub struct SendOutcome {
    pub provider_message_id: Option<String>,
}

/// Pluggable email transport. SMTP (the default, works with any standard
/// relay and needs no third-party service) and, later, Resend implement
/// this; `EmailService` composes a message then hands it to whichever
/// transport is configured.
#[async_trait]
pub trait EmailTransport: Send + Sync {
    async fn send(&self, msg: &OutboundEmailMessage<'_>) -> Result<SendOutcome, String>;
    fn is_configured(&self) -> bool;
    /// Stable identifier for status reporting (`"smtp"` | `"resend"`).
    fn provider_name(&self) -> &'static str;
}

/// SMTP transport via lettre. The default provider.
pub struct SmtpEmailTransport {
    config: EmailConfig,
}

impl SmtpEmailTransport {
    pub fn new(config: EmailConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl EmailTransport for SmtpEmailTransport {
    async fn send(&self, msg: &OutboundEmailMessage<'_>) -> Result<SendOutcome, String> {
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }
        let message = build_outbound_message(&self.config, msg)?;
        let mailer = build_smtp_mailer(&self.config)?;
        mailer
            .send(&message)
            .map_err(|e| format!("Failed to send ticket reply: {e}"))?;
        Ok(SendOutcome {
            provider_message_id: None,
        })
    }

    fn is_configured(&self) -> bool {
        self.config.is_configured()
    }

    fn provider_name(&self) -> &'static str {
        "smtp"
    }
}

/// Email service for sending emails
pub struct EmailService {
    config: EmailConfig,
    /// The configured transport (SMTP by default). Composition stays in
    /// `EmailService`; the transport only performs the provider hand-off.
    transport: Arc<dyn EmailTransport>,
}

impl EmailService {
    /// Create a new email service with the given configuration
    pub fn new(config: EmailConfig) -> Self {
        // SMTP is the default transport and the self-host standard.
        // Provider selection (e.g. Resend) is added here in a later stage.
        let transport: Arc<dyn EmailTransport> = Arc::new(SmtpEmailTransport::new(config.clone()));
        Self { config, transport }
    }

    /// Create email service from environment variables, selecting the
    /// transport. SMTP is the default and the self-host standard; Resend
    /// is used when `EMAIL_PROVIDER=resend` (or, as a convenience, when
    /// `RESEND_API_KEY` is set and no provider is named).
    pub fn from_env() -> Result<Self, String> {
        let provider = env::var("EMAIL_PROVIDER")
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let use_resend =
            provider == "resend" || (provider.is_empty() && env::var("RESEND_API_KEY").is_ok());

        if use_resend {
            let api_key = env::var("RESEND_API_KEY")
                .map_err(|_| "RESEND_API_KEY is required when EMAIL_PROVIDER=resend".to_string())?;
            let from_name = env::var("SMTP_FROM_NAME")
                .or_else(|_| env::var("RESEND_FROM_NAME"))
                .unwrap_or_else(|_| "Nosdesk".to_string());
            let from_email = env::var("RESEND_FROM_EMAIL")
                .or_else(|_| env::var("SMTP_FROM_EMAIL"))
                .map_err(|_| {
                    "RESEND_FROM_EMAIL or SMTP_FROM_EMAIL is required for Resend".to_string()
                })?;
            // The config carries the shared sender + enabled flag; the
            // SMTP-specific fields stay empty under the Resend transport.
            let config = EmailConfig {
                smtp_host: String::new(),
                smtp_port: 0,
                smtp_username: String::new(),
                smtp_password: String::new(),
                from_name: from_name.clone(),
                from_email: from_email.clone(),
                enabled: true,
                security: SmtpSecurity::Tls,
            };
            let transport: Arc<dyn EmailTransport> =
                Arc::new(crate::utils::email_resend::ResendEmailTransport::new(
                    api_key, from_name, from_email,
                ));
            return Ok(Self { config, transport });
        }

        let config = EmailConfig::from_env()?;
        Ok(Self::new(config))
    }

    /// True when the active transport can send (SMTP credentials present,
    /// or a Resend API key configured). Provider-aware, unlike the raw
    /// SMTP-centric `EmailConfig::is_configured`.
    pub fn is_configured(&self) -> bool {
        self.transport.is_configured()
    }

    /// Active provider identifier for admin status reporting
    /// (`"smtp"` | `"resend"`).
    pub fn provider_name(&self) -> &'static str {
        self.transport.provider_name()
    }

    /// Generate a unique RFC Message-ID for a direct (non-queued) send.
    /// Queue rows carry their own stable id; direct sends (test email,
    /// guest confirmation) mint one here so the header is always present.
    fn generate_message_id(&self) -> String {
        let domain = self
            .config
            .from_email
            .rsplit('@')
            .next()
            .filter(|d| !d.is_empty())
            .unwrap_or("nosdesk.local");
        format!("{}@{}", uuid::Uuid::now_v7(), domain)
    }

    /// Send a simple text email through the configured transport (SMTP or
    /// Resend), so direct sends are provider-agnostic like the queue path.
    pub async fn send_text_email(&self, to: &str, subject: &str, body: &str) -> Result<(), String> {
        let message_id = self.generate_message_id();
        let outbound = OutboundEmailMessage {
            to,
            subject,
            body_text: body,
            body_html: None,
            message_id: &message_id,
            in_reply_to: None,
            references: &[],
            auto_submitted: false,
        };
        self.send_outbound(&outbound).await.map(|_| ())
    }

    /// Send an HTML email through the configured transport. A plaintext
    /// alternative is derived from the HTML so the message is a proper
    /// multipart/alternative rather than HTML-only.
    pub async fn send_html_email(
        &self,
        to: &str,
        subject: &str,
        html_body: &str,
    ) -> Result<(), String> {
        let text = crate::utils::content::html_to_plaintext(html_body);
        let message_id = self.generate_message_id();
        let outbound = OutboundEmailMessage {
            to,
            subject,
            body_text: &text,
            body_html: Some(html_body),
            message_id: &message_id,
            in_reply_to: None,
            references: &[],
            auto_submitted: false,
        };
        self.send_outbound(&outbound).await.map(|_| ())
    }

    /// Send a test email to verify configuration
    pub async fn send_test_email(&self, to: &str, branding: &EmailBranding) -> Result<(), String> {
        let subject = format!("{} Test Email", branding.app_name);
        let body = format!(
            "This is a test email from {}.\n\n\
            If you received this email, your email configuration is working correctly.\n\n\
            SMTP Server: {}\n\
            SMTP Port: {}\n\
            From: {} <{}>",
            branding.app_name,
            self.config.smtp_host,
            self.config.smtp_port,
            self.config.from_name,
            self.config.from_email
        );

        self.send_text_email(to, &subject, &body).await
    }

    /// Access the underlying SMTP configuration. Needed by callers
    /// (the queue path) that want to derive the From-email domain
    /// for outbound Message-IDs without re-reading env.
    pub fn config(&self) -> &EmailConfig {
        &self.config
    }

    /// Render the password-reset email without sending. Returns
    /// `(subject, body_html, body_text)`. Used by both the legacy
    /// fire-and-forget `send_password_reset_email` and the
    /// queued `transactional_email::enqueue_password_reset` so the
    /// HTML, copy, and plain-text alternative stay aligned across
    /// the two delivery paths.
    pub fn compose_password_reset(
        &self,
        user_name: &str,
        reset_token: &str,
        branding: &EmailBranding,
        locale: &unic_langid::LanguageIdentifier,
    ) -> (String, String, String) {
        let reset_link = format!("{}/reset-password?token={}", branding.base_url, reset_token);
        let template = EmailTemplate::new(branding);
        let tr = |key: &str, args: &[(&str, fluent_bundle::FluentValue<'static>)]| {
            crate::utils::i18n::tr_with(locale, key, args)
        };

        // For HTML interpolation we pass HTML-escaped variable
        // values into Fluent; the `<strong>` markers around them
        // come from the FTL value itself. For plaintext we pass
        // the raw values because there's no HTML context.
        let name_html = escape_html(user_name);
        let app_html = escape_html(&branding.app_name);

        let greeting = tr(
            "password-reset-greeting",
            &[("name", name_html.clone().into())],
        );
        let intro = tr("password-reset-intro", &[("app", app_html.clone().into())]);
        let action_prompt = tr("password-reset-action-prompt", &[]);
        let title = tr("password-reset-title", &[]);
        let cta_label = tr("password-reset-cta-label", &[]);
        let footer = tr("password-reset-footer", &[]);
        let notice_items_owned: Vec<String> = [
            "password-reset-notice-expiry",
            "password-reset-notice-single-use",
            "password-reset-notice-never-share",
            "password-reset-notice-account-security",
        ]
        .iter()
        .map(|key| tr(key, &[]))
        .collect();
        let notice_items: Vec<&str> = notice_items_owned.iter().map(String::as_str).collect();

        let content = format!(
            r#"<p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                {greeting}
            </p>
            <p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                {intro}
            </p>
            <p style="margin: 0 0 8px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                {action_prompt}
            </p>"#
        );

        let html_body = template.build(
            &title,
            &branding.primary_color,
            &content,
            &cta_label,
            &reset_link,
            &branding.primary_color,
            NoticeType::Warning,
            &notice_items,
            &footer,
            locale,
        );

        let subject = tr(
            "password-reset-subject",
            &[("app", branding.app_name.clone().into())],
        );

        // Plain-text alternative comes from one multi-line FTL
        // value so translators see the whole prose at once
        // (instead of stitching together six fragments).
        let body_text = tr(
            "password-reset-body-text",
            &[
                ("name", user_name.to_string().into()),
                ("app", branding.app_name.clone().into()),
                ("link", reset_link.clone().into()),
            ],
        );

        (subject, html_body, body_text)
    }

    /// Render the invitation email without sending. See
    /// `compose_password_reset` for the rationale.
    pub fn compose_invitation(
        &self,
        user_name: &str,
        invitation_token: &str,
        branding: &EmailBranding,
        invited_by: &str,
        locale: &unic_langid::LanguageIdentifier,
    ) -> (String, String, String) {
        let setup_link = format!(
            "{}/accept-invitation?token={}",
            branding.base_url, invitation_token
        );
        let template = EmailTemplate::new(branding);
        let tr = |key: &str, args: &[(&str, fluent_bundle::FluentValue<'static>)]| {
            crate::utils::i18n::tr_with(locale, key, args)
        };

        // HTML-escape user-supplied / branding strings before
        // handing them to Fluent for the HTML keys; plaintext
        // path passes raw values.
        let name_html = escape_html(user_name);
        let app_html = escape_html(&branding.app_name);
        let by_html = escape_html(invited_by);

        let title = tr("invitation-title", &[("app", app_html.clone().into())]);
        let greeting = tr("invitation-greeting", &[("name", name_html.clone().into())]);
        let intro = tr(
            "invitation-intro",
            &[
                ("app", app_html.clone().into()),
                ("by", by_html.clone().into()),
            ],
        );
        let action_prompt = tr("invitation-action-prompt", &[]);
        let cta_label = tr("invitation-cta-label", &[]);
        let footer = tr("invitation-footer", &[]);
        let notice_items_owned: Vec<String> = [
            "invitation-notice-expiry",
            "invitation-notice-create-password",
            "invitation-notice-strong-password",
            "invitation-notice-unexpected",
        ]
        .iter()
        .map(|key| tr(key, &[]))
        .collect();
        let notice_items: Vec<&str> = notice_items_owned.iter().map(String::as_str).collect();

        let content = format!(
            r#"<p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                {greeting}
            </p>
            <p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                {intro}
            </p>
            <p style="margin: 0 0 8px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                {action_prompt}
            </p>"#
        );

        // Green/success accent — the invitation flow is positive
        // by intent (joining a workspace), distinct from password
        // reset's warning amber.
        let welcome_color = "#059669";

        let html_body = template.build(
            &title,
            welcome_color,
            &content,
            &cta_label,
            &setup_link,
            welcome_color,
            NoticeType::Info,
            &notice_items,
            &footer,
            locale,
        );

        let subject = tr(
            "invitation-subject",
            &[("app", branding.app_name.clone().into())],
        );

        let body_text = tr(
            "invitation-body-text",
            &[
                ("name", user_name.to_string().into()),
                ("app", branding.app_name.clone().into()),
                ("by", invited_by.to_string().into()),
                ("link", setup_link.clone().into()),
            ],
        );

        (subject, html_body, body_text)
    }

    /// Send a confirmation email for a guest ticket submission. The link
    /// uses the same accept-invitation flow as a normal invitation, but the
    /// copy is tailored to the ticket-submission context — the email is
    /// framed as "confirm your submission" rather than "welcome / set up
    /// your account", which is what the submitter actually requested.
    pub async fn send_guest_ticket_confirmation_email(
        &self,
        to: &str,
        user_name: &str,
        invitation_token: &str,
        branding: &EmailBranding,
    ) -> Result<(), String> {
        // Guest confirmation predates the inbound-locale plumbing.
        // Fall back to DEFAULT_LOCALE; once guest channels carry an
        // Accept-Language hint we can thread it through.
        let locale =
            unic_langid::LanguageIdentifier::from_str(crate::utils::locale::DEFAULT_LOCALE)
                .expect("DEFAULT_LOCALE parses");
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }

        let confirm_link = format!(
            "{}/accept-invitation?token={}",
            branding.base_url, invitation_token
        );
        let template = EmailTemplate::new(branding);

        let content = format!(
            r#"<p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                Hi <strong>{}</strong>,
            </p>
            <p style="margin: 0 0 8px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                Thanks for submitting a ticket to <strong>{}</strong>. Confirm your email to release it to our team:
            </p>"#,
            escape_html(user_name),
            escape_html(&branding.app_name)
        );

        // Use the app's accent color for the guest confirmation flow so it
        // reads as "your helpdesk" rather than a generic invitation.
        let accent = &branding.primary_color;

        // No recipient locale to resolve from at this point — the
        // guest has just submitted a form, no account exists yet,
        // so we default to "en". A future commit could resolve
        // the inbound `Accept-Language` header or
        // site_settings.default_locale here.
        let html_body = template.build(
            "Confirm your ticket submission",
            accent,
            &content,
            "Confirm email & send ticket",
            &confirm_link,
            accent,
            NoticeType::Info,
            &[
                "Link expires in <strong>7 days</strong>",
                "Confirming also gives you access to your ticket portal to track progress and reply",
            ],
            "If you didn't submit a ticket, you can safely ignore this email, no account will be created.",
            &locale,
        );

        let subject = format!("Confirm your ticket submission to {}", branding.app_name);
        self.send_html_email(to, &subject, &html_body).await
    }

    /// Send a technician's reply to a ticket as an email. Sets the
    /// threading headers (`Message-ID`, `In-Reply-To`, `References`) so
    /// the recipient's mail client groups the conversation correctly,
    /// and so a future inbound reply from the customer can be routed
    /// back to this ticket by the threading cascade.
    ///
    /// `message_id` is the ID we want to stamp on this outbound message
    /// (no angle brackets — those are added here). It should be produced
    /// by [`crate::services::channels::threading::format_outbound_message_id`]
    /// so the inbound pipeline recognizes it later.
    ///
    /// `in_reply_to` / `references` must already carry angle brackets;
    /// they are joined verbatim into the header values.
    pub async fn send_ticket_reply(
        &self,
        outbound: OutboundEmailMessage<'_>,
    ) -> Result<(), String> {
        self.send_outbound(&outbound).await.map(|_| ())
    }

    /// Send and return the transport outcome (the provider message id for
    /// Resend, `None` for SMTP). The queue worker uses this to persist the
    /// provider id for webhook correlation; `send_ticket_reply` is the
    /// thin wrapper for callers that don't need the id (auto-ack, IMAP).
    pub async fn send_outbound(
        &self,
        outbound: &OutboundEmailMessage<'_>,
    ) -> Result<SendOutcome, String> {
        self.transport.send(outbound).await
    }

    /// Test-only: lets unit tests inspect the serialized form (headers,
    /// body parts) without a live transport. The production path builds
    /// the message inside the transport via `build_outbound_message`.
    #[cfg(test)]
    pub(crate) fn build_ticket_reply_message(
        &self,
        outbound: &OutboundEmailMessage<'_>,
    ) -> Result<Message, String> {
        build_outbound_message(&self.config, outbound)
    }

    /// Render the notification email without sending. Returns
    /// `(body_html, body_text)`; the caller already has the subject
    /// (notifications synthesise it from the notification type).
    ///
    /// `title` and `body` are user-authored content (entity title /
    /// comment body) and stay verbatim — we don't machine-translate
    /// what humans wrote. Only the connector copy (From label, CTA,
    /// footer) gets translated against the recipient's locale.
    pub fn compose_notification(
        &self,
        title: &str,
        body: &str,
        actor_name: &str,
        cta_url: &str,
        branding: &EmailBranding,
        locale: &unic_langid::LanguageIdentifier,
    ) -> (String, String) {
        let template = EmailTemplate::new(branding);
        let tr = |key: &str, args: &[(&str, fluent_bundle::FluentValue<'static>)]| {
            crate::utils::i18n::tr_with(locale, key, args)
        };

        let from_row = tr(
            "notif-from-row",
            &[("actor", escape_html(actor_name).into())],
        );
        let content = format!(
            r#"<p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">{}</p>
            <p style="margin: 0 0 24px 0; color: #6b7280; font-size: 14px;">{}</p>"#,
            escape_html(body),
            from_row,
        );

        let button_label = tr(
            "notif-cta-view-in",
            &[("app", branding.app_name.clone().into())],
        );
        let footer = tr("notif-footer-preferences", &[]);
        let html_body = template.build(
            title,
            &branding.primary_color,
            &content,
            &button_label,
            cta_url,
            &branding.primary_color,
            NoticeType::Info,
            &[],
            &footer,
            locale,
        );

        let body_text = tr(
            "notif-body-text",
            &[
                ("title", title.to_string().into()),
                ("body", body.to_string().into()),
                ("actor", actor_name.to_string().into()),
                ("app", branding.app_name.clone().into()),
                ("cta", cta_url.to_string().into()),
            ],
        );

        (html_body, body_text)
    }
}

/// Parameters for [`EmailService::send_ticket_reply`]. Keeps the argument
/// list short as the channel abstraction grows (attachments land here in
/// task #20).
pub struct OutboundEmailMessage<'a> {
    pub to: &'a str,
    pub subject: &'a str,
    pub body_text: &'a str,
    pub body_html: Option<&'a str>,
    /// Generated by the threading helper, NOT wrapped in angle brackets.
    pub message_id: &'a str,
    /// Parent message's `Message-ID` with its `<...>` wrapper.
    pub in_reply_to: Option<&'a str>,
    /// Full ancestor chain, each entry already wrapped in `<...>`.
    pub references: &'a [String],
    /// `true` when this is a system-authored automatic response (the
    /// initial "we got your ticket" auto-ack). Causes RFC 3834 loop-
    /// prevention headers to be emitted so the recipient's OOO /
    /// auto-responder won't bounce back and trigger a ping-pong.
    pub auto_submitted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_config_disabled_by_default() {
        // Clear environment variables
        env::remove_var("SMTP_ENABLED");
        env::remove_var("SMTP_HOST");

        let config = EmailConfig::from_env().unwrap();
        assert!(!config.enabled);
        assert!(!config.is_configured());
    }

    #[test]
    fn test_from_mailbox_formatting() {
        let config = EmailConfig {
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            smtp_username: "user@example.com".to_string(),
            smtp_password: "password".to_string(),
            from_name: "Test App".to_string(),
            from_email: "noreply@example.com".to_string(),
            enabled: true,
            security: SmtpSecurity::StartTls,
        };

        let mailbox = config.from_mailbox().unwrap();
        assert_eq!(mailbox.to_string(), "Test App <noreply@example.com>");
    }

    // ---------- send_ticket_reply message construction ----------
    //
    // These tests exercise the serialized Message so the threading
    // headers match what `services::channels::threading` expects when
    // the customer's reply comes back in. No SMTP transport involved.

    fn svc() -> EmailService {
        EmailService::new(EmailConfig {
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            smtp_username: "u".into(),
            smtp_password: "p".into(),
            from_name: "Support".into(),
            from_email: "support@yourco.com".into(),
            enabled: true,
            security: SmtpSecurity::StartTls,
        })
    }

    fn rendered(msg: &Message) -> String {
        String::from_utf8(msg.formatted()).expect("valid utf-8")
    }

    #[test]
    fn ticket_reply_sets_message_id_with_angle_brackets() {
        let msg = svc()
            .build_ticket_reply_message(&OutboundEmailMessage {
                to: "alice@example.com",
                subject: "[#42] Re: Printer fire",
                body_text: "On it",
                body_html: None,
                message_id: "ticket-42.comment-7.deadbeef@yourco.com",
                in_reply_to: None,
                references: &[],
                auto_submitted: false,
            })
            .unwrap();
        assert!(
            rendered(&msg).contains("Message-ID: <ticket-42.comment-7.deadbeef@yourco.com>"),
            "expected Message-ID header with brackets: {}",
            rendered(&msg)
        );
    }

    #[test]
    fn ticket_reply_omits_threading_headers_when_no_parent() {
        let msg = svc()
            .build_ticket_reply_message(&OutboundEmailMessage {
                to: "alice@example.com",
                subject: "[#42] New case",
                body_text: "hi",
                body_html: None,
                message_id: "ticket-42.comment-1.aaaaaaaa@yourco.com",
                in_reply_to: None,
                references: &[],
                auto_submitted: false,
            })
            .unwrap();
        let dump = rendered(&msg);
        assert!(!dump.contains("In-Reply-To:"));
        assert!(!dump.contains("References:"));
    }

    #[test]
    fn ticket_reply_writes_in_reply_to_and_references() {
        let refs = vec!["<first@x>".to_string(), "<second@x>".to_string()];
        let msg = svc()
            .build_ticket_reply_message(&OutboundEmailMessage {
                to: "alice@example.com",
                subject: "[#42] Re: thread",
                body_text: "reply",
                body_html: None,
                message_id: "ticket-42.comment-3.cafef00d@yourco.com",
                in_reply_to: Some("<second@x>"),
                references: &refs,
                auto_submitted: false,
            })
            .unwrap();
        let dump = rendered(&msg);
        assert!(dump.contains("In-Reply-To: <second@x>"), "dump:\n{dump}");
        assert!(
            dump.contains("References: <first@x> <second@x>"),
            "dump:\n{dump}"
        );
    }

    #[test]
    fn ticket_reply_builds_multipart_when_html_provided() {
        let msg = svc()
            .build_ticket_reply_message(&OutboundEmailMessage {
                to: "alice@example.com",
                subject: "[#42] hi",
                body_text: "plain body",
                body_html: Some("<p>html body</p>"),
                message_id: "ticket-42.comment-5.f00dbabe@yourco.com",
                in_reply_to: None,
                references: &[],
                auto_submitted: false,
            })
            .unwrap();
        let dump = rendered(&msg);
        assert!(dump.contains("multipart/alternative"), "dump:\n{dump}");
        assert!(dump.contains("plain body"), "dump:\n{dump}");
        assert!(dump.contains("<p>html body</p>"), "dump:\n{dump}");
    }

    #[test]
    fn send_ticket_reply_refuses_when_disabled() {
        let disabled = EmailService::new(EmailConfig {
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_username: String::new(),
            smtp_password: String::new(),
            from_name: String::new(),
            from_email: String::new(),
            enabled: false,
            security: SmtpSecurity::StartTls,
        });
        let outbound = OutboundEmailMessage {
            to: "x@example.com",
            subject: "hi",
            body_text: "hi",
            body_html: None,
            message_id: "ticket-1.comment-1.aa@host",
            in_reply_to: None,
            references: &[],
            auto_submitted: false,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(disabled.send_ticket_reply(outbound))
            .unwrap_err();
        assert!(err.contains("not configured"), "unexpected error: {err}");
    }
}
