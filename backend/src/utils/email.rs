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
    /// Fully-resolved anti-phishing footer line, or `None` to omit it.
    /// Resolution (toggle, template selection, placeholder
    /// substitution) happens in `utils::email_branding`; the template
    /// layer here only renders the string when present.
    pub security_note: Option<String>,
}

impl Default for EmailBranding {
    fn default() -> Self {
        Self {
            app_name: "Nosdesk".to_string(),
            logo_url: None,
            primary_color: "#FF6B1A".to_string(),
            base_url: env::var("FRONTEND_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_string()),
            security_note: None,
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
            primary_color: primary_color.unwrap_or_else(|| "#FF6B1A".to_string()),
            base_url,
            security_note: None,
        }
    }

    /// Generate lighter shade of primary color for backgrounds.
    /// Retained from the previous design (the new stationery layout uses
    /// fixed paper tokens rather than a tinted accent), kept for callers
    /// that derive a soft background from the brand color.
    #[allow(dead_code)]
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

// ===========================================================================
// Email template layer ("fine-stationery" design)
//
// The layer is split into three pieces so compose_* functions never
// hand-write inline-styled HTML:
//
//   * `Block`    — a single body element (paragraph, sub-heading, note).
//                  Constructed via the free helpers `text`, `heading`,
//                  `note`. Each helper emits the correct inline-styled,
//                  dark-mode-aware HTML *once*.
//   * `Notice`   — the bulleted "security notes" box, with a `NoticeType`
//                  title resolved from the FTL catalogue.
//   * `EmailLayout` — the params struct for the whole letter. It owns the
//                  headline, the body blocks, an optional CTA, an optional
//                  notice, and an optional sign-off. `#[derive(Default)]`
//                  means call sites use named fields and adding a field
//                  never breaks them.
//
// `EmailTemplate::render` owns ALL chrome exactly once: `<head>` + dark-
// mode `<style>`, hidden preheader, logo letterhead + hairline rule, CTA
// (MSO/Outlook VML bulletproof button), notice, sign-off, footer (automated
// notice + © + Help) and the anti-phishing trust line.
// ===========================================================================

// --- Design tokens (light) -------------------------------------------------
// Dark-mode equivalents live in the `<style>` block via the `.nd-*` classes;
// these are the inline (light) values email clients without media-query
// support fall back to.
const C_PAPER: &str = "#f6f2ea";
const C_HEAD: &str = "#1f1a15";
const C_BODY: &str = "#4a443c";
const C_MUTED: &str = "#8c8378";
const C_FAINT: &str = "#a89f93";
const C_LINK: &str = "#be4607";
const C_HAIR: &str = "#e6ddcd";
const C_NOTERULE: &str = "#d8d0c2";
const C_FALLBACK_BG: &str = "#efe9dd";
const C_STRONG: &str = "#5b5349";
const C_CTA: &str = "#ff6b1a";

/// A single rendered body block. The inline styling is baked in by the
/// `text` / `heading` / `note` helpers so compose_* code only ever deals
/// in semantics, never in `<p style="...">`.
pub struct Block(String);

/// A body paragraph. `html` is trusted markup (already escaped at the Rust
/// boundary by the caller, with `<strong>` emphasis coming from the FTL
/// value). Styled as the letter body text.
pub fn text(html: impl Into<String>) -> Block {
    Block(format!(
        r#"<p class="nd-body" style="margin:0 0 18px 0;color:{body};font-size:15px;line-height:1.72;">{content}</p>"#,
        body = C_BODY,
        content = html.into(),
    ))
}

/// A muted secondary line (e.g. the notification "From: …" row).
pub fn muted(html: impl Into<String>) -> Block {
    Block(format!(
        r#"<p class="nd-muted" style="margin:0 0 18px 0;color:{muted};font-size:13px;line-height:1.6;">{content}</p>"#,
        muted = C_MUTED,
        content = html.into(),
    ))
}

/// A body sub-heading. Smaller than the letter headline; rarely needed,
/// kept for completeness so compose_* never reaches for raw `<h*>`.
#[allow(dead_code)]
pub fn heading(html: impl Into<String>) -> Block {
    Block(format!(
        r#"<p class="nd-head" style="margin:0 0 12px 0;color:{head};font-size:17px;line-height:1.4;font-weight:600;">{content}</p>"#,
        head = C_HEAD,
        content = html.into(),
    ))
}

/// A quiet aside with a subtle grey left rule — the "didn't request this?" note.
pub fn note(html: impl Into<String>) -> Block {
    Block(format!(
        r#"<table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%" style="margin:0 0 18px 0;"><tr>
                <td class="nd-noterule" style="border-left:2px solid {rule};padding:2px 0 2px 16px;">
                  <p class="nd-muted" style="margin:0;color:{muted};font-size:13px;line-height:1.6;">{content}</p>
                </td>
              </tr></table>"#,
        rule = C_NOTERULE,
        muted = C_MUTED,
        content = html.into(),
    ))
}

/// Call-to-action button. `label` is plain text (escaped here); `url` is a
/// trusted, already-constructed link.
pub struct Cta {
    pub label: String,
    pub url: String,
}

/// The bulleted security-notes box.
pub struct Notice {
    pub kind: NoticeType,
    /// Each item is trusted markup (FTL value with optional `<strong>`).
    pub items: Vec<String>,
}

/// Params for a full letter. Construct with named fields and `..Default::default()`
/// so adding a field never breaks an existing call site.
#[derive(Default)]
pub struct EmailLayout<'a> {
    /// The `<title>` and `<h1>` headline (plain text, escaped on render).
    pub headline: &'a str,
    /// Inbox-snippet preheader (plain text, escaped on render). Falls back
    /// to "{headline} — {app_name}" when empty.
    pub preheader: &'a str,
    /// Ordered body blocks built via `text` / `heading` / `note` / `muted`.
    pub body: Vec<Block>,
    /// Optional call-to-action button (+ paste-the-link fallback).
    pub cta: Option<Cta>,
    /// Optional bulleted security-notes box, rendered after the body.
    pub notice: Option<Notice>,
    /// Optional sign-off (e.g. "With care,"). The `app_name` team line is
    /// appended automatically when set.
    pub signoff: Option<&'a str>,
}

/// Email template builder for consistent, branded emails
struct EmailTemplate<'a> {
    branding: &'a EmailBranding,
}

impl<'a> EmailTemplate<'a> {
    fn new(branding: &'a EmailBranding) -> Self {
        Self { branding }
    }

    /// Resolve the logo URL against `base_url` (relative paths get the
    /// base prefix; absolute `http(s)` URLs pass through).
    fn logo_full_url(&self, logo_url: &str) -> String {
        if logo_url.starts_with("http") {
            logo_url.to_string()
        } else {
            format!("{}{}", self.branding.base_url, logo_url)
        }
    }

    /// The letterhead: a logo `<img>` when branding carries one, otherwise
    /// the wordmark fallback in brand orange.
    fn build_logo_section(&self) -> String {
        match &self.branding.logo_url {
            Some(logo_url) if !logo_url.is_empty() => {
                let full_url = self.logo_full_url(logo_url);
                format!(
                    r#"<img src="{src}" width="150" height="27" alt="{alt}" style="display:block;width:150px;height:27px;border:0;outline:none;" />"#,
                    src = escape_html(&full_url),
                    alt = escape_html(&self.branding.app_name),
                )
            }
            _ => {
                // Text wordmark fallback in brand orange (not the old blue).
                format!(
                    r#"<span style="display:inline-block;color:{cta};font-size:24px;font-weight:700;letter-spacing:-0.02em;">{name}</span>"#,
                    cta = C_CTA,
                    name = escape_html(&self.branding.app_name),
                )
            }
        }
    }

    /// Render the bulleted notice box, or empty string when no items.
    fn build_notice_section(
        &self,
        notice: &Notice,
        locale: &unic_langid::LanguageIdentifier,
    ) -> String {
        if notice.items.is_empty() {
            return String::new();
        }

        let title = crate::utils::i18n::tr(locale, notice.kind.title_key());

        let items_html: String = notice
            .items
            .iter()
            .map(|item| {
                format!(
                    r#"<li class="nd-muted" style="margin:0 0 7px 0;color:{muted};font-size:13px;line-height:1.6;">{item}</li>"#,
                    muted = C_MUTED,
                )
            })
            .collect();

        format!(
            r#"<table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%"><tr>
                <td class="nd-noterule" style="border-left:2px solid {rule};padding:2px 0 2px 16px;">
                  <p class="nd-strong" style="margin:0 0 10px 0;color:{strong};font-size:13px;font-weight:600;">{title}</p>
                  <ul style="margin:0;padding:0 0 0 18px;">{items_html}</ul>
                </td>
              </tr></table>"#,
            rule = C_NOTERULE,
            strong = C_STRONG,
        )
    }

    /// Render the bulletproof CTA (VML for Outlook/MSO, anchor elsewhere)
    /// plus the paste-the-link fallback.
    fn build_cta_section(&self, cta: &Cta, locale: &unic_langid::LanguageIdentifier) -> String {
        let fallback_prompt = crate::utils::i18n::tr(locale, "email-link-fallback-prompt");
        format!(
            r#"<tr>
            <td class="nd-pad" style="padding:30px 8px 0 8px;">
              <!--[if mso]>
              <v:roundrect xmlns:v="urn:schemas-microsoft-com:vml" xmlns:w="urn:schemas-microsoft-com:office:word" href="{url}" style="height:46px;v-text-anchor:middle;width:200px;" arcsize="18%" strokecolor="{cta}" fillcolor="{cta}">
              <w:anchorlock/><center style="color:#ffffff;font-family:sans-serif;font-size:15px;font-weight:600;">{label}</center>
              </v:roundrect>
              <![endif]-->
              <!--[if !mso]><!-->
              <a href="{url}" target="_blank" role="button" style="display:inline-block;background-color:{cta};color:#ffffff;font-size:15px;font-weight:600;padding:13px 30px;border-radius:8px;letter-spacing:0.01em;">{label}</a>
              <!--<![endif]-->
            </td>
          </tr>
          <tr>
            <td class="nd-pad" style="padding:22px 8px 0 8px;">
              <p class="nd-muted" style="margin:0 0 8px 0;color:{muted};font-size:12.5px;line-height:1.5;">{fallback_prompt}</p>
              <p class="nd-fallback" style="margin:0;padding:11px 14px;background-color:{fallback_bg};border-radius:7px;word-break:break-all;font-size:12px;font-family:'SF Mono',Monaco,Consolas,monospace;">
                <a class="nd-link" href="{url}" style="color:{link};">{url_text}</a>
              </p>
            </td>
          </tr>"#,
            url = cta.url,
            url_text = escape_html(&cta.url),
            label = escape_html(&cta.label),
            cta = C_CTA,
            muted = C_MUTED,
            link = C_LINK,
            fallback_bg = C_FALLBACK_BG,
        )
    }

    /// Build the complete HTML letter. `EmailLayout` carries every variable
    /// part; this owns all the chrome. `locale` lands on `<html lang>` so
    /// screen readers announce the right rules and clients skip auto-translate.
    fn render(&self, layout: EmailLayout<'_>, locale: &unic_langid::LanguageIdentifier) -> String {
        let lang = locale.to_string();
        let app_name = escape_html(&self.branding.app_name);
        let logo_html = self.build_logo_section();

        let preheader = if layout.preheader.is_empty() {
            format!("{} — {}", layout.headline, self.branding.app_name)
        } else {
            layout.preheader.to_string()
        };

        let body_html: String = layout.body.iter().map(|b| b.0.as_str()).collect();

        let cta_html = layout
            .cta
            .as_ref()
            .map(|c| self.build_cta_section(c, locale))
            .unwrap_or_default();

        let notice_html = layout
            .notice
            .as_ref()
            .map(|n| {
                let inner = self.build_notice_section(n, locale);
                if inner.is_empty() {
                    String::new()
                } else {
                    format!(
                        r#"<tr><td class="nd-pad" style="padding:28px 8px 0 8px;">{inner}</td></tr>"#
                    )
                }
            })
            .unwrap_or_default();

        let signoff_html = layout
            .signoff
            .map(|s| {
                format!(
                    r#"<tr><td class="nd-pad" style="padding:32px 8px 0 8px;">
              <p class="nd-body" style="margin:0;color:{body};font-size:15px;line-height:1.72;">{signoff}<br><span class="nd-strong" style="color:{head};">{team}</span></p>
            </td></tr>"#,
                    body = C_BODY,
                    head = C_HEAD,
                    signoff = escape_html(s),
                    team = escape_html(&self.branding.app_name),
                )
            })
            .unwrap_or_default();

        let automated_notice = crate::utils::i18n::tr(locale, "email-footer-automated");
        let help_label = crate::utils::i18n::tr(locale, "email-footer-help");
        let help_url = format!("{}/support", self.branding.base_url);

        // The anti-phishing line is opt-in and admin-authored (or the
        // localized default), resolved upstream into `security_note`.
        // Escaped here since it carries no trusted markup, and omitted
        // entirely when the workspace has the note turned off.
        let security_note_html = self
            .branding
            .security_note
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|note| {
                format!(
                    r#"<p class="nd-faint" style="margin:0;color:{faint};font-size:11.5px;line-height:1.65;">{note}</p>"#,
                    faint = C_FAINT,
                    note = escape_html(note),
                )
            })
            .unwrap_or_default();

        format!(
            r#"<!DOCTYPE html>
<html lang="{lang}" style="margin:0;padding:0;">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta http-equiv="X-UA-Compatible" content="IE=edge">
  <meta name="color-scheme" content="light dark">
  <meta name="supported-color-schemes" content="light dark">
  <title>{title}</title>
  <!--[if mso]>
  <noscript><xml><o:OfficeDocumentSettings><o:PixelsPerInch>96</o:PixelsPerInch></o:OfficeDocumentSettings></xml></noscript>
  <![endif]-->
  <style>
    @media (prefers-color-scheme: dark) {{
      .nd-paper   {{ background:#0b0a08 !important; }}
      .nd-head    {{ color:#f4f1ea !important; }}
      .nd-body    {{ color:#cfc8bd !important; }}
      .nd-muted   {{ color:#9a9082 !important; }}
      .nd-faint   {{ color:#7a7263 !important; }}
      .nd-link    {{ color:#ff9d57 !important; }}
      .nd-fallback{{ background:#16140f !important; }}
      .nd-hair    {{ border-color:#2a2620 !important; }}
      .nd-noterule{{ border-color:#3a352d !important; }}
      .nd-strong  {{ color:#e6ded0 !important; }}
    }}
    a {{ text-decoration:none; }}
    @media only screen and (max-width:560px) {{
      .nd-pad   {{ padding-left:14px !important; padding-right:14px !important; }}
      .nd-outer {{ padding-left:12px !important; padding-right:12px !important; padding-top:36px !important; padding-bottom:32px !important; }}
    }}
  </style>
</head>
<body class="nd-paper" style="margin:0;padding:0;background-color:{paper};font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif;-webkit-font-smoothing:antialiased;">
  <div style="display: none; max-height:0;overflow:hidden;opacity:0;">{preheader}</div>

  <table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%" class="nd-paper" style="background-color:{paper};">
    <tr>
      <td align="center" class="nd-outer" style="padding:56px 24px 44px 24px;">

        <table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%" style="max-width:512px;margin:0 auto;">

          <!-- Letterhead: wordmark + hairline rule -->
          <tr><td align="center" style="padding:0 0 6px 0;">
            {logo_html}
          </td></tr>
          <tr><td align="center" style="padding:18px 0 0 0;">
            <table role="presentation" cellspacing="0" cellpadding="0" border="0"><tr>
              <td class="nd-hair" style="width:38px;border-top:1px solid {hair};font-size:0;line-height:0;">&nbsp;</td>
            </tr></table>
          </td></tr>

          <!-- Letter body -->
          <tr>
            <td class="nd-pad" style="padding:40px 8px 0 8px;">
              <h1 class="nd-head" style="margin:0 0 22px 0;color:{head};font-size:22px;line-height:1.35;font-weight:600;letter-spacing:-0.01em;">{title}</h1>
              {body_html}
            </td>
          </tr>

          <!-- CTA -->
          {cta_html}

          <!-- Notice -->
          {notice_html}

          <!-- Sign-off -->
          {signoff_html}

          <!-- Footer -->
          <tr>
            <td class="nd-pad" style="padding:40px 8px 0 8px;">
              <table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%"><tr>
                <td class="nd-hair" style="border-top:1px solid {hair};font-size:0;line-height:0;">&nbsp;</td>
              </tr></table>
              <p class="nd-faint" style="margin:18px 0 4px 0;color:{faint};font-size:12px;line-height:1.5;">{automated_notice}</p>
              <p class="nd-faint" style="margin:0 0 16px 0;color:{faint};font-size:12px;">&copy; {year} {app_name} &nbsp;&middot;&nbsp; <a class="nd-link" href="{help_url}" style="color:{faint};">{help_label}</a></p>
              {security_note_html}
            </td>
          </tr>

        </table>
      </td>
    </tr>
  </table>
</body>
</html>"#,
            title = escape_html(layout.headline),
            preheader = escape_html(&preheader),
            paper = C_PAPER,
            head = C_HEAD,
            hair = C_HAIR,
            faint = C_FAINT,
            year = chrono::Utc::now().format("%Y"),
        )
    }
}

/// Notice type for email templates. Picks the title key for the
/// security-notes box and is preserved from the previous design.
#[derive(Clone, Copy)]
pub enum NoticeType {
    Warning,
    #[allow(dead_code)]
    Critical,
    Info,
    #[allow(dead_code)]
    Success,
}

impl NoticeType {
    fn title_key(self) -> &'static str {
        match self {
            NoticeType::Warning => "email-notice-security",
            NoticeType::Critical => "email-notice-security-critical",
            NoticeType::Info => "email-notice-getting-started",
            NoticeType::Success => "email-notice-success",
        }
    }
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
    let builder = match config.security {
        SmtpSecurity::Tls => SmtpTransport::relay(&config.smtp_host)
            .map_err(|e| format!("Failed to create SMTP transport: {e}"))?,
        SmtpSecurity::StartTls => SmtpTransport::starttls_relay(&config.smtp_host)
            .map_err(|e| format!("Failed to create SMTP transport: {e}"))?,
        // Plain transport for local test servers (Greenmail, Mailpit): no
        // TLS, no auth.
        SmtpSecurity::Plaintext => SmtpTransport::builder_dangerous(&config.smtp_host),
    };
    let builder = builder.port(config.smtp_port);

    // Only authenticate when the connection can actually carry credentials.
    // lettre refuses PLAIN/LOGIN over an unencrypted link, and attaching
    // credentials to a server that offers no AUTH (the dev Mailpit sidecar,
    // where SMTP_SECURITY=plaintext but the .env username/password are still
    // present) makes the send fail with "No compatible authentication
    // mechanism was found". Plaintext is local-test only, so skip auth there;
    // also skip when no credentials are configured (open / IP-allowlisted relay).
    let authenticate = config.security != SmtpSecurity::Plaintext
        && !config.smtp_username.is_empty()
        && !config.smtp_password.is_empty();
    let builder = if authenticate {
        builder.credentials(Credentials::new(
            config.smtp_username.clone(),
            config.smtp_password.clone(),
        ))
    } else {
        builder
    };

    Ok(builder.build())
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
    // RFC 3834 + Exchange loop-prevention headers for system-authored mail.
    if let Some(value) = outbound.auto_submitted {
        builder = builder
            .header(AutoSubmitted(value.to_string()))
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
            auto_submitted: None,
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
            auto_submitted: None,
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
        let notice_items: Vec<String> = [
            "password-reset-notice-expiry",
            "password-reset-notice-single-use",
            "password-reset-notice-never-share",
            "password-reset-notice-account-security",
        ]
        .iter()
        .map(|key| tr(key, &[]))
        .collect();

        let html_body = template.render(
            EmailLayout {
                headline: &title,
                body: vec![text(greeting), text(intro), text(action_prompt)],
                cta: Some(Cta {
                    label: cta_label,
                    url: reset_link.clone(),
                }),
                notice: Some(Notice {
                    kind: NoticeType::Warning,
                    items: notice_items,
                }),
                signoff: None,
                preheader: &footer,
                ..Default::default()
            },
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
        let notice_items: Vec<String> = [
            "invitation-notice-expiry",
            "invitation-notice-create-password",
            "invitation-notice-strong-password",
            "invitation-notice-unexpected",
        ]
        .iter()
        .map(|key| tr(key, &[]))
        .collect();

        let html_body = template.render(
            EmailLayout {
                headline: &title,
                body: vec![text(greeting), text(intro), text(action_prompt)],
                cta: Some(Cta {
                    label: cta_label,
                    url: setup_link.clone(),
                }),
                notice: Some(Notice {
                    kind: NoticeType::Info,
                    items: notice_items,
                }),
                signoff: None,
                preheader: &footer,
                ..Default::default()
            },
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
        if !self.config.is_configured() {
            return Err("Email is not configured".to_string());
        }
        let (subject, html_body) =
            self.compose_guest_ticket_confirmation(user_name, invitation_token, branding);
        self.send_html_email(to, &subject, &html_body).await
    }

    /// Render the guest ticket-confirmation email without sending. Returns
    /// `(subject, html_body)`; the plain-text alternative is derived from the
    /// HTML by `send_html_email`. Split out so the preview harness can render
    /// it without a transport.
    pub fn compose_guest_ticket_confirmation(
        &self,
        user_name: &str,
        invitation_token: &str,
        branding: &EmailBranding,
    ) -> (String, String) {
        // Guest confirmation predates the inbound-locale plumbing.
        // Fall back to DEFAULT_LOCALE; once guest channels carry an
        // Accept-Language hint we can thread it through.
        let locale =
            unic_langid::LanguageIdentifier::from_str(crate::utils::locale::DEFAULT_LOCALE)
                .expect("DEFAULT_LOCALE parses");

        let confirm_link = format!(
            "{}/accept-invitation?token={}",
            branding.base_url, invitation_token
        );
        let template = EmailTemplate::new(branding);

        // No recipient locale to resolve from at this point — the
        // guest has just submitted a form, no account exists yet, so
        // we default to DEFAULT_LOCALE. The copy here predates the
        // i18n plumbing for this flow and stays hardcoded English;
        // a future commit could resolve the inbound `Accept-Language`
        // header or site_settings.default_locale here.
        let greeting = format!("Hi <strong>{}</strong>,", escape_html(user_name));
        let intro = format!(
            "Thanks for submitting a ticket to <strong>{}</strong>. Confirm your email to release it to our team:",
            escape_html(&branding.app_name)
        );

        let html_body = template.render(
            EmailLayout {
                headline: "Confirm your ticket submission",
                body: vec![
                    text(greeting),
                    text(intro),
                    note("If you didn't submit a ticket, you can safely ignore this email, no account will be created."),
                ],
                cta: Some(Cta {
                    label: "Confirm email & send ticket".to_string(),
                    url: confirm_link.clone(),
                }),
                notice: Some(Notice {
                    kind: NoticeType::Info,
                    items: vec![
                        "Link expires in <strong>7 days</strong>".to_string(),
                        "Confirming also gives you access to your ticket portal to track progress and reply".to_string(),
                    ],
                }),
                signoff: None,
                preheader: "",
                ..Default::default()
            },
            &locale,
        );

        let subject = format!("Confirm your ticket submission to {}", branding.app_name);
        (subject, html_body)
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

        let button_label = tr(
            "notif-cta-view-in",
            &[("app", branding.app_name.clone().into())],
        );
        let footer = tr("notif-footer-preferences", &[]);
        let html_body = template.render(
            EmailLayout {
                headline: title,
                body: vec![text(escape_html(body)), muted(from_row)],
                cta: Some(Cta {
                    label: button_label,
                    url: cta_url.to_string(),
                }),
                notice: None,
                signoff: None,
                preheader: &footer,
                ..Default::default()
            },
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
    /// The RFC 3834 `Auto-Submitted` value when this is a system-authored
    /// automatic message: `"auto-replied"` for a direct reply (the
    /// "we got your ticket" auto-ack), `"auto-generated"` for other
    /// automated mail (password reset, invitation, notification). `None`
    /// for ordinary mail. When set, the loop-prevention headers
    /// (`Auto-Submitted` + `X-Auto-Response-Suppress`) are emitted so the
    /// recipient's OOO / auto-responder won't bounce back and ping-pong.
    pub auto_submitted: Option<&'a str>,
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
                auto_submitted: None,
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
                auto_submitted: None,
            })
            .unwrap();
        let dump = rendered(&msg);
        assert!(!dump.contains("In-Reply-To:"));
        assert!(!dump.contains("References:"));
    }

    #[test]
    fn auto_submitted_value_reaches_the_wire() {
        // The loop-prevention headers must ship with the producer's actual
        // value, on the built message, not just in the queue row. Regression
        // guard for the worker honouring only "auto-replied" and silently
        // dropping the headers for "auto-generated" transactional mail.
        let base = |auto: Option<&'static str>| {
            rendered(
                &svc()
                    .build_ticket_reply_message(&OutboundEmailMessage {
                        to: "alice@example.com",
                        subject: "[#42] hi",
                        body_text: "hi",
                        body_html: None,
                        message_id: "ticket-42.comment-1.aaaaaaaa@yourco.com",
                        in_reply_to: None,
                        references: &[],
                        auto_submitted: auto,
                    })
                    .unwrap(),
            )
        };

        let generated = base(Some("auto-generated"));
        assert!(
            generated.contains("Auto-Submitted: auto-generated"),
            "transactional mail must carry Auto-Submitted: auto-generated: {generated}"
        );
        assert!(generated.contains("X-Auto-Response-Suppress: All"));

        let replied = base(Some("auto-replied"));
        assert!(replied.contains("Auto-Submitted: auto-replied"));

        let human = base(None);
        assert!(
            !human.contains("Auto-Submitted:"),
            "ordinary mail must not carry loop-prevention headers"
        );
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
                auto_submitted: None,
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
                auto_submitted: None,
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
            auto_submitted: None,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(disabled.send_ticket_reply(outbound))
            .unwrap_err();
        assert!(err.contains("not configured"), "unexpected error: {err}");
    }

    // ---------- email design preview harness ----------
    //
    // Renders each compose_* with representative sample data + default
    // branding and writes the HTML to `target/email-preview/<name>.html`
    // so the "fine-stationery" design can be eyeballed in a browser
    // without sending. Pure render — no network, no DB.

    #[test]
    fn render_email_previews() {
        use std::str::FromStr;

        let svc = svc();
        let mut branding = EmailBranding::default(); // default brand orange, no logo
                                                     // Opt-in anti-phishing footer, resolved upstream in
                                                     // production. Set here so the preview shows the line.
        branding.security_note = Some(
            "Acme only ever emails you from acme.example.com. We will never ask \
             for your password or a login code by email."
                .to_string(),
        );
        let locale = unic_langid::LanguageIdentifier::from_str("en-US").unwrap();

        let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("email-preview");
        std::fs::create_dir_all(&out_dir).expect("create preview dir");

        let write = |name: &str, html: &str| {
            std::fs::write(out_dir.join(format!("{name}.html")), html)
                .unwrap_or_else(|e| panic!("write {name}: {e}"));
        };

        let (_subj, html, _text) =
            svc.compose_password_reset("Alex", "EXAMPLE-RESET-TOKEN", &branding, &locale);
        write("password-reset", &html);

        let (_subj, html, _text) =
            svc.compose_invitation("Alex", "EXAMPLE-INVITE-TOKEN", &branding, "Kyle", &locale);
        write("invitation", &html);

        let (_subj, html) =
            svc.compose_guest_ticket_confirmation("Alex", "EXAMPLE-GUEST-TOKEN", &branding);
        write("guest-ticket-confirmation", &html);

        let (html, _text) = svc.compose_notification(
            "New comment on: Printer fire",
            "It's still burning. Can someone take a look?",
            "Kyle",
            "https://desk.example.com/tickets/42",
            &branding,
            &locale,
        );
        write("notification", &html);

        // Sanity: every preview file exists and is non-trivial.
        for name in [
            "password-reset",
            "invitation",
            "guest-ticket-confirmation",
            "notification",
        ] {
            let p = out_dir.join(format!("{name}.html"));
            let content = std::fs::read_to_string(&p).expect("preview readable");
            assert!(content.contains("<!DOCTYPE html>"), "{name} is a full doc");
            assert!(
                content.contains("acme.example.com"),
                "{name} renders the configured security note"
            );
        }
    }
}
