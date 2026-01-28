use lettre::{
    Message, SmtpTransport, Transport,
    message::{header::ContentType, Mailbox},
    transport::smtp::authentication::Credentials,
};
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
        // Pre-compute the light color to avoid lifetime issues
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

        Ok(Self {
            smtp_host,
            smtp_port,
            smtp_username,
            smtp_password,
            from_name,
            from_email,
            enabled,
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

        // Use starttls_relay for explicit STARTTLS on port 587
        let transport = SmtpTransport::starttls_relay(&self.config.smtp_host)
            .map_err(|e| format!("Failed to create SMTP transport: {e}"))?
            .port(self.config.smtp_port)
            .credentials(creds)
            .build();

        Ok(transport)
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
        self.send_html_email(to, &subject, &html_body).await
    }

    /// Send an MFA reset email with branding
    pub async fn send_mfa_reset_email(
        &self,
        to: &str,
        user_name: &str,
        reset_token: &str,
        branding: &EmailBranding,
    ) -> Result<(), String> {
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }

        let reset_link = format!("{}/mfa-recovery?token={}", branding.base_url, reset_token);
        let template = EmailTemplate::new(branding);

        let content = format!(
            r#"<p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                Hello <strong>{}</strong>,
            </p>
            <p style="margin: 0 0 16px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                We received a request to recover access to your Multi-Factor Authentication (MFA) protected account. If you didn't make this request, please contact your system administrator immediately.
            </p>
            <p style="margin: 0 0 8px 0; color: #374151; font-size: 16px; line-height: 1.6;">
                Click the button below to access your MFA recovery session:
            </p>"#,
            escape_html(user_name)
        );

        // Use red for critical security emails
        let critical_color = "#dc2626";

        let html_body = template.build(
            "MFA Account Recovery",
            critical_color,
            &content,
            "Manage MFA Settings",
            &reset_link,
            critical_color,
            NoticeType::Critical,
            &[
                "This recovery link will expire in <strong>15 minutes</strong>",
                "This link can only be used <strong>once</strong>",
                "You'll have a limited session to manage your MFA settings only",
                "Never share this link with anyone",
                "If you didn't request this recovery, your account may be compromised",
            ],
            "For security concerns, please contact your system administrator immediately.",
        );

        let subject = format!("MFA Account Recovery - {}", branding.app_name);
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
        self.send_html_email(to, &subject, &html_body).await
    }
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
        };

        let mailbox = config.from_mailbox().unwrap();
        assert_eq!(mailbox.to_string(), "Test App <noreply@example.com>");
    }
}
