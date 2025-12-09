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
            .map_err(|e| format!("Invalid from address: {}", e))
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
            .map_err(|e| format!("Failed to create SMTP transport: {}", e))?
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
            .map_err(|e| format!("Invalid recipient email: {}", e))?;

        let email = Message::builder()
            .from(self.config.from_mailbox()?)
            .to(to_mailbox)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .map_err(|e| format!("Failed to build email: {}", e))?;

        let mailer = self.build_transport()?;

        mailer.send(&email)
            .map_err(|e| format!("Failed to send email: {}", e))?;

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
            .map_err(|e| format!("Invalid recipient email: {}", e))?;

        let email = Message::builder()
            .from(self.config.from_mailbox()?)
            .to(to_mailbox)
            .subject(subject)
            .header(ContentType::TEXT_HTML)
            .body(html_body.to_string())
            .map_err(|e| format!("Failed to build email: {}", e))?;

        let mailer = self.build_transport()?;

        mailer.send(&email)
            .map_err(|e| format!("Failed to send email: {}", e))?;

        Ok(())
    }

    /// Send a test email to verify configuration
    pub async fn send_test_email(&self, to: &str) -> Result<(), String> {
        let subject = "Nosdesk Test Email";
        let body = format!(
            "This is a test email from Nosdesk.\n\n\
            If you received this email, your email configuration is working correctly.\n\n\
            SMTP Server: {}\n\
            SMTP Port: {}\n\
            From: {} <{}>",
            self.config.smtp_host,
            self.config.smtp_port,
            self.config.from_name,
            self.config.from_email
        );

        self.send_text_email(to, subject, &body).await
    }

    /// Send a password reset email
    pub async fn send_password_reset_email(
        &self,
        to: &str,
        user_name: &str,
        reset_token: &str,
        base_url: &str,
    ) -> Result<(), String> {
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }

        let reset_link = format!("{}/reset-password?token={}", base_url, reset_token);

        let html_body = format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 600px;
            margin: 0 auto;
            padding: 20px;
        }}
        .container {{
            background-color: #ffffff;
            border-radius: 8px;
            padding: 30px;
            box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
        }}
        .header {{
            text-align: center;
            margin-bottom: 30px;
        }}
        .header h1 {{
            color: #2563eb;
            margin: 0;
            font-size: 24px;
        }}
        .content {{
            margin-bottom: 30px;
        }}
        .button {{
            display: inline-block;
            background-color: #2563eb;
            color: #ffffff !important;
            text-decoration: none;
            padding: 12px 30px;
            border-radius: 6px;
            font-weight: 500;
            text-align: center;
            margin: 20px 0;
        }}
        .button:hover {{
            background-color: #1d4ed8;
        }}
        .warning {{
            background-color: #fef3c7;
            border-left: 4px solid #f59e0b;
            padding: 15px;
            margin: 20px 0;
            border-radius: 4px;
        }}
        .footer {{
            margin-top: 30px;
            padding-top: 20px;
            border-top: 1px solid #e5e7eb;
            font-size: 14px;
            color: #6b7280;
            text-align: center;
        }}
        .code {{
            background-color: #f3f4f6;
            padding: 2px 6px;
            border-radius: 3px;
            font-family: monospace;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>Password Reset Request</h1>
        </div>

        <div class="content">
            <p>Hello {user_name},</p>

            <p>We received a request to reset your password for your Nosdesk account. If you didn't make this request, you can safely ignore this email.</p>

            <p>To reset your password, click the button below:</p>

            <div style="text-align: center;">
                <a href="{reset_link}" class="button">Reset Password</a>
            </div>

            <p>Or copy and paste this link into your browser:</p>
            <p style="word-break: break-all;"><a href="{reset_link}">{reset_link}</a></p>

            <div class="warning">
                <strong>⚠️ Security Notice:</strong>
                <ul style="margin: 10px 0; padding-left: 20px;">
                    <li>This link will expire in <strong>1 hour</strong></li>
                    <li>This link can only be used <strong>once</strong></li>
                    <li>Never share this link with anyone</li>
                    <li>If you didn't request this reset, please secure your account immediately</li>
                </ul>
            </div>
        </div>

        <div class="footer">
            <p>This is an automated message from Nosdesk. Please do not reply to this email.</p>
            <p>If you have any questions or concerns, please contact your system administrator.</p>
        </div>
    </div>
</body>
</html>"#,
            user_name = escape_html(user_name),
            reset_link = reset_link
        );

        let subject = "Reset Your Nosdesk Password";

        self.send_html_email(to, subject, &html_body).await
    }

    /// Send an MFA reset email
    pub async fn send_mfa_reset_email(
        &self,
        to: &str,
        user_name: &str,
        reset_token: &str,
        base_url: &str,
    ) -> Result<(), String> {
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }

        let reset_link = format!("{}/mfa-recovery?token={}", base_url, reset_token);

        let html_body = format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 600px;
            margin: 0 auto;
            padding: 20px;
        }}
        .container {{
            background-color: #ffffff;
            border-radius: 8px;
            padding: 30px;
            box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
        }}
        .header {{
            text-align: center;
            margin-bottom: 30px;
        }}
        .header h1 {{
            color: #dc2626;
            margin: 0;
            font-size: 24px;
        }}
        .content {{
            margin-bottom: 30px;
        }}
        .button {{
            display: inline-block;
            background-color: #dc2626;
            color: #ffffff !important;
            text-decoration: none;
            padding: 12px 30px;
            border-radius: 6px;
            font-weight: 500;
            text-align: center;
            margin: 20px 0;
        }}
        .button:hover {{
            background-color: #b91c1c;
        }}
        .warning {{
            background-color: #fee2e2;
            border-left: 4px solid #dc2626;
            padding: 15px;
            margin: 20px 0;
            border-radius: 4px;
        }}
        .footer {{
            margin-top: 30px;
            padding-top: 20px;
            border-top: 1px solid #e5e7eb;
            font-size: 14px;
            color: #6b7280;
            text-align: center;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🔐 MFA Reset Request</h1>
        </div>

        <div class="content">
            <p>Hello {user_name},</p>

            <p>We received a request to recover access to your Multi-Factor Authentication (MFA) protected account. If you didn't make this request, please contact your system administrator immediately.</p>

            <p>Click the button below to access your MFA recovery session. You'll be able to disable or reconfigure your MFA settings:</p>

            <div style="text-align: center;">
                <a href="{reset_link}" class="button">Manage MFA Settings</a>
            </div>

            <p>Or copy and paste this link into your browser:</p>
            <p style="word-break: break-all;"><a href="{reset_link}">{reset_link}</a></p>

            <div class="warning">
                <strong>⚠️ Critical Security Notice:</strong>
                <ul style="margin: 10px 0; padding-left: 20px;">
                    <li>This recovery link will expire in <strong>15 minutes</strong></li>
                    <li>This link can only be used <strong>once</strong></li>
                    <li>You'll have a limited session to manage your MFA settings only</li>
                    <li>Never share this link with anyone</li>
                    <li>If you didn't request this recovery, your account may be compromised</li>
                </ul>
            </div>
        </div>

        <div class="footer">
            <p>This is an automated message from Nosdesk. Please do not reply to this email.</p>
            <p>For security concerns, please contact your system administrator immediately.</p>
        </div>
    </div>
</body>
</html>"#,
            user_name = escape_html(user_name),
            reset_link = reset_link
        );

        let subject = "MFA Account Recovery - Nosdesk";

        self.send_html_email(to, subject, &html_body).await
    }

    /// Send a user invitation email for new account setup
    pub async fn send_invitation_email(
        &self,
        to: &str,
        user_name: &str,
        invitation_token: &str,
        base_url: &str,
        invited_by: &str,
    ) -> Result<(), String> {
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }

        let setup_link = format!("{}/accept-invitation?token={}", base_url, invitation_token);

        let html_body = format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 600px;
            margin: 0 auto;
            padding: 20px;
        }}
        .container {{
            background-color: #ffffff;
            border-radius: 8px;
            padding: 30px;
            box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
        }}
        .header {{
            text-align: center;
            margin-bottom: 30px;
        }}
        .header h1 {{
            color: #059669;
            margin: 0;
            font-size: 24px;
        }}
        .content {{
            margin-bottom: 30px;
        }}
        .button {{
            display: inline-block;
            background-color: #059669;
            color: #ffffff !important;
            text-decoration: none;
            padding: 12px 30px;
            border-radius: 6px;
            font-weight: 500;
            text-align: center;
            margin: 20px 0;
        }}
        .button:hover {{
            background-color: #047857;
        }}
        .info {{
            background-color: #ecfdf5;
            border-left: 4px solid #059669;
            padding: 15px;
            margin: 20px 0;
            border-radius: 4px;
        }}
        .footer {{
            margin-top: 30px;
            padding-top: 20px;
            border-top: 1px solid #e5e7eb;
            font-size: 14px;
            color: #6b7280;
            text-align: center;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>Welcome to Nosdesk!</h1>
        </div>

        <div class="content">
            <p>Hello {user_name},</p>

            <p>You've been invited to join Nosdesk by <strong>{invited_by}</strong>. To complete your account setup and create your password, click the button below:</p>

            <div style="text-align: center;">
                <a href="{setup_link}" class="button">Set Up Your Account</a>
            </div>

            <p>Or copy and paste this link into your browser:</p>
            <p style="word-break: break-all;"><a href="{setup_link}">{setup_link}</a></p>

            <div class="info">
                <strong>ℹ️ Getting Started:</strong>
                <ul style="margin: 10px 0; padding-left: 20px;">
                    <li>This invitation link will expire in <strong>7 days</strong></li>
                    <li>You'll need to create a password during setup</li>
                    <li>Choose a strong password with at least 8 characters</li>
                    <li>If you didn't expect this invitation, you can safely ignore this email</li>
                </ul>
            </div>
        </div>

        <div class="footer">
            <p>This is an automated message from Nosdesk. Please do not reply to this email.</p>
            <p>If you have any questions, please contact your system administrator.</p>
        </div>
    </div>
</body>
</html>"#,
            user_name = escape_html(user_name),
            invited_by = escape_html(invited_by),
            setup_link = setup_link
        );

        let subject = "You've Been Invited to Nosdesk - Set Up Your Account";

        self.send_html_email(to, subject, &html_body).await
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
