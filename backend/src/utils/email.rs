use lettre::{
    Message, SmtpTransport, Transport,
    message::{
        header::{ContentType, Header, HeaderName, HeaderValue, InReplyTo, MessageId, References},
        Mailbox, MultiPart,
    },
    transport::smtp::authentication::Credentials,
};

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
            base_url: env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:3000".to_string()),
        }
    }
}

impl EmailBranding {
    /// Create branding config from site settings
    pub fn new(app_name: String, logo_url: Option<String>, primary_color: Option<String>, base_url: String) -> Self {
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

    /// Build complete HTML email with branding
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
    ) -> String {
        let logo_html = self.build_logo_section();
        let notice_html = self.build_notice_section(notice_type, notice_items);

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
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
                                Or copy and paste this link into your browser:
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
                                &copy; {year} {app_name}. All rights reserved.
                            </p>
                        </td>
                    </tr>

                </table>

                <!-- Unsubscribe / Help links -->
                <table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%" style="max-width: 600px; margin: 24px auto 0;">
                    <tr>
                        <td style="text-align: center;">
                            <p style="margin: 0; color: #9ca3af; font-size: 12px;">
                                This is an automated message. Please do not reply directly to this email.
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
    fn build_notice_section(&self, notice_type: NoticeType, items: &[&str]) -> String {
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

        let title = match notice_type {
            NoticeType::Warning => "Security Notice",
            NoticeType::Critical => "Critical Security Notice",
            NoticeType::Info => "Getting Started",
            NoticeType::Success => "Success",
        };

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

        let smtp_host = env::var("SMTP_HOST")
            .map_err(|_| "SMTP_HOST not configured".to_string())?;

        let smtp_port = env::var("SMTP_PORT")
            .unwrap_or_else(|_| "587".to_string())
            .parse::<u16>()
            .map_err(|_| "Invalid SMTP_PORT".to_string())?;

        let smtp_username = env::var("SMTP_USERNAME")
            .map_err(|_| "SMTP_USERNAME not configured".to_string())?;

        let smtp_password = env::var("SMTP_PASSWORD")
            .map_err(|_| "SMTP_PASSWORD not configured".to_string())?;

        let from_name = env::var("SMTP_FROM_NAME")
            .unwrap_or_else(|_| "Nosdesk".to_string());

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

/// Email service for sending emails
pub struct EmailService {
    config: EmailConfig,
}

impl EmailService {
    /// Create a new email service with the given configuration
    pub fn new(config: EmailConfig) -> Self {
        Self { config }
    }

    /// Create email service from environment variables
    pub fn from_env() -> Result<Self, String> {
        let config = EmailConfig::from_env()?;
        Ok(Self::new(config))
    }

    /// Build SMTP transport from configuration
    fn build_transport(&self) -> Result<SmtpTransport, String> {
        let creds = Credentials::new(
            self.config.smtp_username.clone(),
            self.config.smtp_password.clone(),
        );

        let builder = match self.config.security {
            SmtpSecurity::Tls => SmtpTransport::relay(&self.config.smtp_host)
                .map_err(|e| format!("Failed to create SMTP transport: {e}"))?,
            SmtpSecurity::StartTls => SmtpTransport::starttls_relay(&self.config.smtp_host)
                .map_err(|e| format!("Failed to create SMTP transport: {e}"))?,
            // Plain transport for local test servers — no TLS, no auth
            // negotiation. Credentials still ride but the connection
            // is cleartext, so we warn loudly in the doc comment on
            // `SmtpSecurity::Plaintext`.
            SmtpSecurity::Plaintext => SmtpTransport::builder_dangerous(&self.config.smtp_host),
        };

        Ok(builder
            .port(self.config.smtp_port)
            .credentials(creds)
            .build())
    }

    /// Send a simple text email
    pub async fn send_text_email(
        &self,
        to: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), String> {
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }

        let to_mailbox: Mailbox = to.parse()
            .map_err(|e| format!("Invalid recipient email: {e}"))?;

        let email = Message::builder()
            .from(self.config.from_mailbox()?)
            .to(to_mailbox)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .map_err(|e| format!("Failed to build email: {e}"))?;

        let mailer = self.build_transport()?;

        mailer.send(&email)
            .map_err(|e| format!("Failed to send email: {e}"))?;

        Ok(())
    }

    /// Send an HTML email
    pub async fn send_html_email(
        &self,
        to: &str,
        subject: &str,
        html_body: &str,
    ) -> Result<(), String> {
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }

        let to_mailbox: Mailbox = to.parse()
            .map_err(|e| format!("Invalid recipient email: {e}"))?;

        let email = Message::builder()
            .from(self.config.from_mailbox()?)
            .to(to_mailbox)
            .subject(subject)
            .header(ContentType::TEXT_HTML)
            .body(html_body.to_string())
            .map_err(|e| format!("Failed to build email: {e}"))?;

        let mailer = self.build_transport()?;

        mailer.send(&email)
            .map_err(|e| format!("Failed to send email: {e}"))?;

        Ok(())
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
    ) -> (String, String, String) {
        let reset_link = format!("{}/reset-password?token={}", branding.base_url, reset_token);
        let template = EmailTemplate::new(branding);

        let content = format!(
            r#"<p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                Hello <strong>{}</strong>,
            </p>
            <p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                We received a request to reset your password for your {} account. If you didn't make this request, you can safely ignore this email.
            </p>
            <p style="margin: 0 0 8px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                To reset your password, click the button below:
            </p>"#,
            escape_html(user_name),
            escape_html(&branding.app_name)
        );

        let html_body = template.build(
            "Password Reset Request",
            &branding.primary_color,
            &content,
            "Reset Password",
            &reset_link,
            &branding.primary_color,
            NoticeType::Warning,
            &[
                "This link will expire in <strong>1 hour</strong>",
                "This link can only be used <strong>once</strong>",
                "Never share this link with anyone",
                "If you didn't request this reset, please secure your account immediately",
            ],
            "If you have any questions, please contact your system administrator.",
        );

        let subject = format!("Reset Your {} Password", branding.app_name);

        // Plain-text alternative — hand-authored rather than HTML→
        // text stripped so the copy reads as deliberate prose and
        // the CTA is a bare URL (the bracket noise an automated
        // strip produces looks like spam to filters).
        let body_text = format!(
            "Hello {name},\n\n\
             We received a request to reset your password for your {app} account. \
             If you didn't make this request, you can safely ignore this email.\n\n\
             To reset your password, open this link in your browser:\n\n\
             {link}\n\n\
             Security notes:\n\
               - This link will expire in 1 hour.\n\
               - This link can only be used once.\n\
               - Never share this link with anyone.\n\
               - If you didn't request this reset, secure your account.\n\n\
             If you have any questions, please contact your system administrator.\n\n\
             — {app}\n",
            name = user_name,
            app = branding.app_name,
            link = reset_link,
        );

        (subject, html_body, body_text)
    }

    /// Send a password reset email with branding
    pub async fn send_password_reset_email(
        &self,
        to: &str,
        user_name: &str,
        reset_token: &str,
        branding: &EmailBranding,
    ) -> Result<(), String> {
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }

        let (subject, html_body, _text_body) = self.compose_password_reset(
            user_name, reset_token, branding,
        );
        self.send_html_email(to, &subject, &html_body).await
    }

    /// Send a user invitation email with branding
    pub async fn send_invitation_email(
        &self,
        to: &str,
        user_name: &str,
        invitation_token: &str,
        branding: &EmailBranding,
        invited_by: &str,
    ) -> Result<(), String> {
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }

        let (subject, html_body, _text_body) = self.compose_invitation(
            user_name, invitation_token, branding, invited_by,
        );
        self.send_html_email(to, &subject, &html_body).await
    }

    /// Render the invitation email without sending. See
    /// `compose_password_reset` for the rationale.
    pub fn compose_invitation(
        &self,
        user_name: &str,
        invitation_token: &str,
        branding: &EmailBranding,
        invited_by: &str,
    ) -> (String, String, String) {
        let setup_link = format!("{}/accept-invitation?token={}", branding.base_url, invitation_token);
        let template = EmailTemplate::new(branding);

        let content = format!(
            r#"<p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                Hello <strong>{}</strong>,
            </p>
            <p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                You've been invited to join <strong>{}</strong> by <strong>{}</strong>.
            </p>
            <p style="margin: 0 0 8px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                To complete your account setup and create your password, click the button below:
            </p>"#,
            escape_html(user_name),
            escape_html(&branding.app_name),
            escape_html(invited_by)
        );

        // Use green/success color for welcome emails
        let welcome_color = "#059669";

        let html_body = template.build(
            &format!("Welcome to {}!", branding.app_name),
            welcome_color,
            &content,
            "Set Up Your Account",
            &setup_link,
            welcome_color,
            NoticeType::Info,
            &[
                "This invitation link will expire in <strong>7 days</strong>",
                "You'll need to create a password during setup",
                "Choose a strong password with at least 8 characters",
                "If you didn't expect this invitation, you can safely ignore this email",
            ],
            "If you have any questions, please contact your system administrator.",
        );

        let subject = format!("You've Been Invited to {} - Set Up Your Account", branding.app_name);

        let body_text = format!(
            "Hello {name},\n\n\
             You've been invited to join {app} by {by}.\n\n\
             To complete your account setup and create your password, open this link:\n\n\
             {link}\n\n\
             A few things to know:\n\
               - This invitation will expire in 7 days.\n\
               - You'll create a password during setup.\n\
               - Choose a strong password with at least 8 characters.\n\
               - If you didn't expect this invitation, you can safely ignore this email.\n\n\
             — {app}\n",
            name = user_name,
            app = branding.app_name,
            by = invited_by,
            link = setup_link,
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
            "If you didn't submit a ticket, you can safely ignore this email — no account will be created.",
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
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }
        let message = self.build_ticket_reply_message(&outbound)?;
        let mailer = self.build_transport()?;
        mailer
            .send(&message)
            .map_err(|e| format!("Failed to send ticket reply: {e}"))?;
        Ok(())
    }

    /// Separated from [`Self::send_ticket_reply`] so unit tests can
    /// inspect the serialized form (headers, body parts) without a live
    /// SMTP transport.
    pub(crate) fn build_ticket_reply_message(
        &self,
        outbound: &OutboundEmailMessage<'_>,
    ) -> Result<Message, String> {
        let to_mailbox: Mailbox = outbound
            .to
            .parse()
            .map_err(|e| format!("Invalid recipient email: {e}"))?;

        let mut builder = Message::builder()
            .from(self.config.from_mailbox()?)
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
        // Emitted only for system-authored messages (auto-ack); tech
        // replies are human and don't carry this.
        if outbound.auto_submitted {
            builder = builder
                .header(AutoSubmitted("auto-replied".to_string()))
                .header(XAutoResponseSuppress("All".to_string()));
        }

        // Prefer multipart/alternative when both text + html are given so
        // clients can pick. Text-only falls back to a single part.
        let message = if let Some(html) = outbound.body_html {
            builder
                .multipart(MultiPart::alternative_plain_html(
                    outbound.body_text.to_string(),
                    html.to_string(),
                ))
                .map_err(|e| format!("Failed to build ticket reply: {e}"))?
        } else {
            builder
                .header(ContentType::TEXT_PLAIN)
                .body(outbound.body_text.to_string())
                .map_err(|e| format!("Failed to build ticket reply: {e}"))?
        };

        Ok(message)
    }

    /// Send a notification email using the branded `EmailTemplate` shell.
    /// Used by the in-app notification delivery channel so notification
    /// emails carry the workspace logo and primary color rather than the
    /// hardcoded blue/white from the legacy inline template.
    pub async fn send_notification_email(
        &self,
        to: &str,
        subject: &str,
        title: &str,
        body: &str,
        actor_name: &str,
        cta_url: &str,
        branding: &EmailBranding,
    ) -> Result<(), String> {
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }
        let (html_body, _text_body) = self.compose_notification(
            title, body, actor_name, cta_url, branding,
        );
        self.send_html_email(to, subject, &html_body).await
    }

    /// Render the notification email without sending. Returns
    /// `(body_html, body_text)`; the caller already has the subject
    /// (notifications synthesise it from the notification type).
    pub fn compose_notification(
        &self,
        title: &str,
        body: &str,
        actor_name: &str,
        cta_url: &str,
        branding: &EmailBranding,
    ) -> (String, String) {
        let template = EmailTemplate::new(branding);
        let content = format!(
            r#"<p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">{}</p>
            <p style="margin: 0 0 24px 0; color: #6b7280; font-size: 14px;"><strong>From:</strong> {}</p>"#,
            escape_html(body),
            escape_html(actor_name),
        );

        let button_label = format!("View in {}", branding.app_name);
        let html_body = template.build(
            title,
            &branding.primary_color,
            &content,
            &button_label,
            cta_url,
            &branding.primary_color,
            NoticeType::Info,
            &[],
            "You're receiving this because of your notification preferences.",
        );

        let body_text = format!(
            "{title}\n\n\
             {body}\n\n\
             From: {actor}\n\n\
             View in {app}: {cta}\n\n\
             — You're receiving this because of your notification preferences in {app}.\n",
            title = title,
            body = body,
            actor = actor_name,
            app = branding.app_name,
            cta = cta_url,
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
        let err = rt.block_on(disabled.send_ticket_reply(outbound)).unwrap_err();
        assert!(err.contains("not configured"), "unexpected error: {err}");
    }
}
