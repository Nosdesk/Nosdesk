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

/// `List-Unsubscribe` (RFC 2369 / 8058) — the unsubscribe URL(s), each in
/// angle brackets. Emitted on opt-out-able notification mail only.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ListUnsubscribe(String);
impl Header for ListUnsubscribe {
    fn name() -> HeaderName {
        HeaderName::new_from_ascii_str("List-Unsubscribe")
    }
    fn parse(s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self(s.into()))
    }
    fn display(&self) -> HeaderValue {
        HeaderValue::new(Self::name(), self.0.clone())
    }
}

/// `List-Unsubscribe-Post` (RFC 8058) — signals one-click support; the only
/// valid value is `List-Unsubscribe=One-Click`. Pairs with an https
/// `List-Unsubscribe` URL so the mail client POSTs it without loading a page.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ListUnsubscribePost;
impl Header for ListUnsubscribePost {
    fn name() -> HeaderName {
        HeaderName::new_from_ascii_str("List-Unsubscribe-Post")
    }
    fn parse(_s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self)
    }
    fn display(&self) -> HeaderValue {
        HeaderValue::new(Self::name(), "List-Unsubscribe=One-Click".to_string())
    }
}
use std::env;
use std::str::FromStr;

use crate::utils::email_logo::{
    parse_hex_color, text_on, wordmark_reads_on, EmailLogo, PAPER_DARK, PAPER_LIGHT,
};

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
pub(crate) fn escape_html(s: &str) -> String {
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
    /// Email copy of the main logo, the one the web app shows on dark
    /// backgrounds and falls back to on light ones.
    pub logo: Option<EmailLogo>,
    /// Email copy of the light-theme logo.
    pub logo_light: Option<EmailLogo>,
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
            app_name: DEFAULT_APP_NAME.to_string(),
            logo: None,
            logo_light: None,
            primary_color: "#FF6B1A".to_string(),
            base_url: env::var("FRONTEND_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_string()),
            security_note: None,
        }
    }
}

impl EmailBranding {
    /// Create branding config from site settings
    pub fn new(app_name: String, primary_color: Option<String>, base_url: String) -> Self {
        Self {
            app_name,
            logo: None,
            logo_light: None,
            primary_color: primary_color.unwrap_or_else(|| "#FF6B1A".to_string()),
            base_url,
            security_note: None,
        }
    }

    /// The brand colour's channels, Nosdesk orange when it isn't a colour.
    fn brand(&self) -> [u8; 3] {
        parse_hex_color(&self.primary_color).unwrap_or(NOSDESK_ORANGE)
    }
}

/// The product name a workspace starts with. Until it is changed, a
/// workspace with no logo signs its mail with the Nosdesk wordmark.
const DEFAULT_APP_NAME: &str = "Nosdesk";
const NOSDESK_ORANGE: [u8; 3] = [0xff, 0x6b, 0x1a];

/// The Nosdesk wordmark (orange, 296 x 54 px), served by every instance from
/// the binary (`handlers::branding::serve_email_asset`), so mail never loads
/// it from nosdesk.com.
const DEFAULT_WORDMARK_PATH: &str = "/email-assets/nosdesk-wordmark.png?v=1";
const DEFAULT_WORDMARK_SIZE: (u32, u32) = (148, 27);

/// Light-text ink for a wordmark on the dark paper.
const C_MARK_ON_DARK: &str = "#f4f1ea";

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Which paper a letter is drawn on. Mail follows the reader's setting; a
/// preview is fixed to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paper {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rendering {
    Mail,
    Preview(Paper),
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

/// The two answers to "is it fixed?" in a resolved-request email. Each opens
/// the request with that answer ready to confirm.
pub struct FeedbackLinks {
    pub fixed: String,
    pub not_fixed: String,
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
    rendering: Rendering,
}

impl<'a> EmailTemplate<'a> {
    fn new(branding: &'a EmailBranding) -> Self {
        Self {
            branding,
            rendering: Rendering::Mail,
        }
    }

    /// A letter for showing in the app, on one paper. Its images load from
    /// the page's own origin, which its content security policy allows.
    fn preview(branding: &'a EmailBranding, paper: Paper) -> Self {
        Self {
            branding,
            rendering: Rendering::Preview(paper),
        }
    }

    /// End a plain-text part the way the letter's footer ends: with the
    /// workspace's security note, when it has one.
    fn finish_text(&self, body: String) -> String {
        match self
            .branding
            .security_note
            .as_deref()
            .filter(|note| !note.trim().is_empty())
        {
            Some(note) => format!("{}\n\n{note}", body.trim_end()),
            None => body,
        }
    }

    /// Where the letter's own images load from: the link origin in mail,
    /// the page's origin in a preview.
    fn asset_origin(&self) -> String {
        match self.rendering {
            Rendering::Mail => origin_of(&self.branding.base_url),
            Rendering::Preview(_) => String::new(),
        }
    }

    /// Resolve the logo URL: absolute `http(s)` URLs pass through; a
    /// relative path is joined to the **origin** of `base_url`, not to
    /// `base_url` itself. On hosted, an agent's link base carries the
    /// workspace slug in its path (`https://app.example/acme`), while
    /// `/uploads/branding/...` is served at the origin root; joining to the
    /// full base produced a URL the SPA answered with `index.html`, and mail
    /// clients showed the alt text in place of the logo.
    fn logo_full_url(&self, logo_url: &str) -> String {
        if logo_url.starts_with("http") {
            logo_url.to_string()
        } else {
            format!("{}{}", self.asset_origin(), logo_url)
        }
    }

    /// The letterhead. The logo's email copy when there is one: on the light
    /// paper the light-theme logo, else the main one; on the dark paper the
    /// main logo, else the light one, as the web app picks them. A logo that
    /// can't be seen on a paper sits on a backing of the other. With no logo,
    /// the Nosdesk wordmark while the workspace carries the Nosdesk name, and
    /// the workspace's name set in its colour otherwise.
    fn build_logo_section(&self) -> String {
        let branding = self.branding;
        let on_light = branding.logo_light.as_ref().or(branding.logo.as_ref());
        let on_dark = branding.logo.as_ref().or(branding.logo_light.as_ref());
        match (on_light, on_dark) {
            (Some(on_light), Some(on_dark)) => {
                let light = self.logo_image(on_light, Paper::Light);
                let dark = self.logo_image(on_dark, Paper::Dark);
                if light == dark {
                    light
                } else {
                    // Clients without `prefers-color-scheme` (Gmail, Outlook)
                    // only ever show the first.
                    format!(
                        r#"<div class="nd-light-only">{light}</div><!--[if !mso]><!--><div class="nd-dark-only" style="display:none;overflow:hidden;max-height:0;max-width:0;mso-hide:all;">{dark}</div><!--<![endif]-->"#
                    )
                }
            }
            _ if branding.app_name.trim() == DEFAULT_APP_NAME => {
                let (width, height) = DEFAULT_WORDMARK_SIZE;
                self.image(
                    &format!("{}{DEFAULT_WORDMARK_PATH}", self.asset_origin()),
                    width,
                    height,
                )
            }
            _ => format!(
                r#"<span class="nd-mark" style="display:inline-block;color:{color};font-size:24px;font-weight:700;letter-spacing:-0.02em;">{name}</span>"#,
                color = self.mark_colors().0,
                name = escape_html(&branding.app_name),
            ),
        }
    }

    /// The colours the workspace's name is set in, on the light and the dark
    /// paper: its brand colour where that reads, plain ink where it doesn't.
    fn mark_colors(&self) -> (String, String) {
        let brand = self.branding.brand();
        let light = if wordmark_reads_on(brand, PAPER_LIGHT) {
            hex(brand)
        } else {
            C_HEAD.to_string()
        };
        let dark = if wordmark_reads_on(brand, PAPER_DARK) {
            hex(brand)
        } else {
            C_MARK_ON_DARK.to_string()
        };
        (light, dark)
    }

    /// An email copy of a logo for one paper, on a backing of the other paper
    /// when it only reads there.
    fn logo_image(&self, logo: &EmailLogo, paper: Paper) -> String {
        let image = self.image(&self.logo_full_url(&logo.url), logo.width, logo.height);
        let (reads_here, reads_on_other, backing) = match paper {
            Paper::Light => (logo.reads_on_light, logo.reads_on_dark, PAPER_DARK),
            Paper::Dark => (logo.reads_on_dark, logo.reads_on_light, PAPER_LIGHT),
        };
        if reads_here || !reads_on_other {
            return image;
        }
        format!(
            r#"<table role="presentation" cellspacing="0" cellpadding="0" border="0" align="center"><tr><td style="background-color:{backing};border-radius:10px;padding:10px 14px;">{image}</td></tr></table>"#,
            backing = hex(backing),
        )
    }

    /// A letterhead image. Its alt text is styled as the workspace's name,
    /// since desktop Outlook blocks images until asked.
    fn image(&self, src: &str, width: u32, height: u32) -> String {
        format!(
            r#"<img class="nd-mark" src="{src}" width="{width}" height="{height}" alt="{alt}" style="display:block;margin:0 auto;width:{width}px;height:{height}px;border:0;outline:none;color:{alt_color};font-size:18px;font-weight:700;line-height:{height}px;text-align:center;" />"#,
            src = escape_html(src),
            alt = escape_html(&self.branding.app_name),
            alt_color = self.mark_colors().0,
        )
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
        // The workspace's colour, with whichever of black or white reads on it.
        let brand = self.branding.brand();
        format!(
            r#"<tr>
            <td class="nd-pad" style="padding:30px 8px 0 8px;">
              <!--[if mso]>
              <v:roundrect xmlns:v="urn:schemas-microsoft-com:vml" xmlns:w="urn:schemas-microsoft-com:office:word" href="{url}" style="height:46px;v-text-anchor:middle;width:200px;" arcsize="18%" strokecolor="{cta}" fillcolor="{cta}">
              <w:anchorlock/><center style="color:{cta_text};font-family:sans-serif;font-size:15px;font-weight:600;">{label}</center>
              </v:roundrect>
              <![endif]-->
              <!--[if !mso]><!-->
              <a href="{url}" target="_blank" role="button" style="display:inline-block;background-color:{cta};color:{cta_text};font-size:15px;font-weight:600;padding:13px 30px;border-radius:8px;letter-spacing:0.01em;">{label}</a>
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
            cta = hex(brand),
            cta_text = text_on(brand),
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

        // The dark paper. Mail switches to it with the reader's setting; a
        // preview is drawn on one paper whatever the viewer's setting is.
        let dark_rules = format!(
            r#"
      .nd-paper   {{ background:{paper_dark} !important; }}
      .nd-head    {{ color:#f4f1ea !important; }}
      .nd-body    {{ color:#cfc8bd !important; }}
      .nd-muted   {{ color:#9a9082 !important; }}
      .nd-faint   {{ color:#7a7263 !important; }}
      .nd-link    {{ color:#ff9d57 !important; }}
      .nd-fallback{{ background:#16140f !important; }}
      .nd-hair    {{ border-color:#2a2620 !important; }}
      .nd-noterule{{ border-color:#3a352d !important; }}
      .nd-strong  {{ color:#e6ded0 !important; }}
      .nd-mark    {{ color:{mark_dark} !important; }}
      .nd-light-only {{ display:none !important; }}
      .nd-dark-only  {{ display:block !important; max-height:none !important; max-width:none !important; overflow:visible !important; }}
"#,
            paper_dark = hex(PAPER_DARK),
            mark_dark = self.mark_colors().1,
        );
        let (color_scheme, dark_css, preview_base) = match self.rendering {
            Rendering::Mail => (
                "light dark",
                format!("@media (prefers-color-scheme: dark) {{{dark_rules}    }}"),
                "",
            ),
            // A preview's links open nothing: the frame that shows it is
            // sandboxed without popups.
            Rendering::Preview(Paper::Light) => {
                ("light", String::new(), r#"<base target="_blank">"#)
            }
            Rendering::Preview(Paper::Dark) => ("dark", dark_rules, r#"<base target="_blank">"#),
        };

        format!(
            r#"<!DOCTYPE html>
<html lang="{lang}" style="margin:0;padding:0;">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta http-equiv="X-UA-Compatible" content="IE=edge">
  <meta name="color-scheme" content="{color_scheme}">
  <meta name="supported-color-schemes" content="{color_scheme}">
  <title>{title}</title>{preview_base}
  <!--[if mso]>
  <noscript><xml><o:OfficeDocumentSettings><o:PixelsPerInch>96</o:PixelsPerInch></o:OfficeDocumentSettings></xml></noscript>
  <![endif]-->
  <style>
    {dark_css}
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
    Critical,
    Info,
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
    /// Connection security. Defaults to [`SmtpSecurity::StartTls`].
    /// [`SmtpSecurity::Plaintext`] sends unencrypted: for a relay on a
    /// trusted local network that offers no STARTTLS, or a local test
    /// server, and never across the internet. It never signs in.
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
    /// No TLS. Only for a relay on a trusted local network that offers no
    /// STARTTLS, or a local test server (Greenmail, Mailpit); never across
    /// the internet. Named `Plaintext` rather than `None` so the variant is
    /// impossible to mistake for "I don't care".
    Plaintext,
}

/// Result of checking whether an SMTP `(port, security)` pair is coherent (B4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmtpCoherence {
    /// Standard, no concerns.
    Ok,
    /// Reachable but worth surfacing (e.g. plaintext submission, port 25).
    Warn(String),
    /// Protocol-impossible: the pair physically cannot complete a connection.
    Error(String),
}

/// Check an SMTP `(port, security)` pair for coherence (B4).
///
/// The load-bearing distinction is implicit-TLS-on-connect (465) vs a STARTTLS
/// upgrade (587), per RFC 8314 / 6409 — the port itself is only a hint. So only
/// the three pairs that physically can't connect are hard [`SmtpCoherence::Error`]s;
/// non-standard ports (2525, custom relays) trust the operator's chosen mode and
/// at most [`SmtpCoherence::Warn`]. This keeps legitimate corporate / alt-port
/// relays working while still catching the cryptic-at-connect-time mistakes.
pub fn check_port_security(port: u16, security: SmtpSecurity) -> SmtpCoherence {
    use SmtpSecurity::*;
    match (port, security) {
        // Port 465 is implicit-TLS-on-connect: a STARTTLS handshake or plaintext
        // EHLO is the wrong first bytes and the connection never establishes.
        (465, StartTls) => SmtpCoherence::Error(
            "Port 465 uses implicit TLS on connect, not STARTTLS. Use port 587 for STARTTLS, \
             or keep 465 and set security to TLS."
                .into(),
        ),
        (465, Plaintext) => SmtpCoherence::Error(
            "Port 465 requires implicit TLS; plaintext is not possible on it.".into(),
        ),
        // Port 587 expects a cleartext EHLO first, then STARTTLS; an immediate
        // TLS handshake is rejected.
        (587, Tls) => SmtpCoherence::Error(
            "Port 587 uses STARTTLS, not implicit TLS. Use port 465 for implicit TLS, \
             or keep 587 and set security to STARTTLS."
                .into(),
        ),
        (465, Tls) | (587, StartTls) => SmtpCoherence::Ok,
        (587, Plaintext) => SmtpCoherence::Warn(
            "Port 587 normally uses STARTTLS; plaintext submission sends mail unencrypted.".into(),
        ),
        // Port 25 is server-to-server relay, not authenticated submission.
        (25, _) => SmtpCoherence::Warn(
            "Port 25 is for server-to-server relay, not authenticated submission; prefer 587 \
             (STARTTLS) or 465 (implicit TLS)."
                .into(),
        ),
        // Any other port: trust the operator's explicit mode, but flag plaintext.
        (_, Plaintext) => SmtpCoherence::Warn(
            "Plaintext SMTP sends mail unencrypted; use TLS or STARTTLS unless this is a trusted \
             local relay."
                .into(),
        ),
        _ => SmtpCoherence::Ok,
    }
}

impl EmailConfig {
    /// Load email configuration from environment variables
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| env::var(name).ok())
    }

    /// [`Self::from_env`] over any source of variables, so tests don't touch
    /// the process environment. A variable set to an empty value counts as
    /// unset, as a compose file's `SMTP_USERNAME=` leaves it.
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let var = |name: &str| lookup(name).filter(|v| !v.trim().is_empty());
        let enabled = var("SMTP_ENABLED")
            .unwrap_or_else(|| "false".to_string())
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

        let smtp_host = var("SMTP_HOST").ok_or_else(|| "SMTP_HOST not configured".to_string())?;

        let smtp_port = var("SMTP_PORT")
            .unwrap_or_else(|| "587".to_string())
            .parse::<u16>()
            .map_err(|_| "Invalid SMTP_PORT".to_string())?;

        // Credentials come as a pair: both to sign in to the relay, neither for
        // a relay that authorises this server by its address.
        let (smtp_username, smtp_password) = match (var("SMTP_USERNAME"), var("SMTP_PASSWORD")) {
            (Some(username), Some(password)) => (username, password),
            (None, None) => (String::new(), String::new()),
            (Some(_), None) => {
                return Err(
                    "SMTP_USERNAME is set but SMTP_PASSWORD isn't: set both to sign in \
                     to the relay, or neither to send without signing in"
                        .to_string(),
                )
            }
            (None, Some(_)) => {
                return Err(
                    "SMTP_PASSWORD is set but SMTP_USERNAME isn't: set both to sign in \
                     to the relay, or neither to send without signing in"
                        .to_string(),
                )
            }
        };

        let from_name = var("SMTP_FROM_NAME").unwrap_or_else(|| "Nosdesk".to_string());

        // The username stands in for the From address only when it is one: an
        // API-key login ("apikey") is no address to send from.
        let from_email = var("SMTP_FROM_EMAIL")
            .or_else(|| Some(smtp_username.clone()).filter(|u| u.contains('@')))
            .ok_or_else(|| {
                "SMTP_FROM_EMAIL not configured: set the address mail is sent from".to_string()
            })?;

        // Optional explicit security selector. Defaults to StartTLS for
        // backward compatibility. `plaintext` is for a relay on a trusted
        // local network that offers no STARTTLS, or a local test server.
        let security = match var("SMTP_SECURITY")
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

        // B4: fail fast at startup on an incoherent port/security pair instead
        // of a cryptic TLS error on the first send. Warnings are logged but
        // don't block boot.
        match check_port_security(smtp_port, security) {
            SmtpCoherence::Error(msg) => {
                return Err(format!(
                    "SMTP_PORT {smtp_port} / SMTP_SECURITY mismatch: {msg}"
                ));
            }
            // Port 25 is where a relay that takes this server by its address
            // listens; the advice to submit on 587 or 465 is for a relay we
            // sign in to.
            SmtpCoherence::Warn(_) if smtp_port == 25 && smtp_username.is_empty() => {}
            SmtpCoherence::Warn(msg) => {
                tracing::warn!(port = smtp_port, "SMTP config warning: {msg}");
            }
            SmtpCoherence::Ok => {}
        }
        if security == SmtpSecurity::Plaintext && !smtp_username.is_empty() {
            tracing::warn!(
                "SMTP_USERNAME and SMTP_PASSWORD are ignored: a plaintext connection never signs in"
            );
        }

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

    /// Whether this can send: turned on, with a host and a From address.
    /// Credentials are optional, since a relay may authorise this server by
    /// its address.
    pub fn is_configured(&self) -> bool {
        self.enabled && !self.smtp_host.trim().is_empty() && !self.from_email.trim().is_empty()
    }

    /// Whether a send signs in to the relay (SMTP AUTH): only with
    /// credentials, over a connection that can carry them. lettre refuses
    /// PLAIN/LOGIN over an unencrypted link, so a plaintext connection never
    /// signs in, and a relay without credentials (one that accepts this server
    /// by its address) isn't asked to.
    pub fn signs_in(&self) -> bool {
        self.security != SmtpSecurity::Plaintext
            && !self.smtp_username.is_empty()
            && !self.smtp_password.is_empty()
    }
}

/// Build a lettre SMTP mailer that TCP-connects to `connect_host` while
/// validating TLS against `tls_domain`. For a trusted relay both are the
/// configured hostname (lettre resolves it). For a tenant relay (`smtp_relay`)
/// `connect_host` is a pre-validated IP and `tls_domain` is the hostname, so the
/// connection goes to the SSRF-vetted address while the certificate / SNI still
/// checks against the hostname. lettre's `relay()`/`starttls_relay()` are just
/// sugar over `builder_dangerous(server).tls(..(TlsParameters::new(domain)))`,
/// so splitting the two is exactly what they do, minus the coupling.
fn build_smtp_mailer_for(
    config: &EmailConfig,
    connect_host: &str,
    tls_domain: &str,
    timeout: Option<std::time::Duration>,
) -> Result<SmtpTransport, String> {
    use lettre::transport::smtp::client::{Tls, TlsParameters};

    let tls_params = || {
        TlsParameters::new(tls_domain.to_string())
            .map_err(|e| format!("Failed to build TLS parameters: {e}"))
    };
    let builder = match config.security {
        SmtpSecurity::Tls => {
            SmtpTransport::builder_dangerous(connect_host).tls(Tls::Wrapper(tls_params()?))
        }
        SmtpSecurity::StartTls => {
            SmtpTransport::builder_dangerous(connect_host).tls(Tls::Required(tls_params()?))
        }
        // Plain transport for local test servers (Greenmail, Mailpit): no
        // TLS, no auth.
        SmtpSecurity::Plaintext => SmtpTransport::builder_dangerous(connect_host),
    };
    let builder = builder.port(config.smtp_port);
    let builder = match timeout {
        Some(t) => builder.timeout(Some(t)),
        None => builder,
    };

    let builder = if config.signs_in() {
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
    // B3: point replies at the channel's polled mailbox when the From diverges.
    if let Some(reply_to) = outbound.reply_to {
        let mailbox: Mailbox = reply_to
            .parse()
            .map_err(|e| format!("Invalid Reply-To address: {e}"))?;
        builder = builder.reply_to(mailbox);
    }
    // B2: one-click unsubscribe on notification mail. The producer only sets
    // this on opt-out-able notification mail, never transactional.
    if let Some(url) = outbound.list_unsubscribe {
        builder = builder
            .header(ListUnsubscribe(format!("<{url}>")))
            .header(ListUnsubscribePost);
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

/// DKIM signing algorithm for a workspace's sending domain. RSA-2048 is the
/// v1 default (universal receiver support); ed25519 is a future option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DkimAlgorithm {
    Rsa,
    Ed25519,
}

impl DkimAlgorithm {
    fn to_lettre(self) -> lettre::message::dkim::DkimSigningAlgorithm {
        match self {
            DkimAlgorithm::Rsa => lettre::message::dkim::DkimSigningAlgorithm::Rsa,
            DkimAlgorithm::Ed25519 => lettre::message::dkim::DkimSigningAlgorithm::Ed25519,
        }
    }
}

/// Per-domain DKIM signing material attached to the SMTP transport. When
/// present, every outbound message is DKIM-signed `d=domain` before the relay
/// hands it off, so DMARC passes via DKIM alignment regardless of the relay's
/// envelope (no SPF alignment needed). The private key is PKCS#1 PEM for `Rsa`
/// or base64 raw bytes for `Ed25519`, matching `algorithm`.
#[derive(Clone)]
pub struct DkimSigner {
    /// DNS selector: `<selector>._domainkey.<domain>`.
    pub selector: String,
    /// The signing domain (the `From` domain); goes in `d=`.
    pub domain: String,
    /// The private key. Never logged (see the `Debug` impl).
    pub private_key: String,
    pub algorithm: DkimAlgorithm,
}

impl std::fmt::Debug for DkimSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DkimSigner")
            .field("selector", &self.selector)
            .field("domain", &self.domain)
            .field("private_key", &"<redacted>")
            .field("algorithm", &self.algorithm)
            .finish()
    }
}

/// Add a `DKIM-Signature` header to `message`, signed with the workspace's key.
/// A key that fails to PARSE surfaces as a send error here; a parsed key that
/// fails to sign panics inside lettre (near-zero for a valid RSA-2048 key).
///
/// Deliberately does NOT use lettre's `default_config`, which signs only
/// From/Subject/To/Date with `simple` header canonicalization. Two changes:
///
/// - **Canonicalization is relaxed/relaxed.** `simple` headers forbid any
///   whitespace/case/folding change to a signed header; a relay (we sign at
///   submission, SES relays afterward) routinely refolds long Subject/References
///   lines, which would invalidate the signature in transit. `relaxed`
///   tolerates that (RFC 6376 §3.4.5), so the signature survives the relay.
///
/// - **Expanded signed-header set.** We cover every header that defines the
///   message's identity and framing, so none can be altered without breaking the
///   signature (RFC 6376 §5.4.1 recommends signing these). Conditional headers
///   (In-Reply-To, References, Reply-To) are listed even when absent: §3.7 treats
///   an `h=` entry with no header as the empty string on both sides, so listing
///   them costs nothing and prevents one being ADDED in transit (oversigning).
fn dkim_sign_message(message: &mut Message, signer: &DkimSigner) -> Result<(), String> {
    use lettre::message::dkim::{
        dkim_sign, DkimCanonicalization, DkimCanonicalizationType, DkimConfig, DkimSigningKey,
    };
    use lettre::message::header::HeaderName;

    let key = DkimSigningKey::new(&signer.private_key, signer.algorithm.to_lettre())
        .map_err(|e| format!("invalid DKIM signing key for {}: {e}", signer.domain))?;

    let headers = [
        "From",
        "Subject",
        "To",
        "Date",
        "Message-ID",
        "MIME-Version",
        "Content-Type",
        "Content-Transfer-Encoding",
        "In-Reply-To",
        "References",
        "Reply-To",
        // RFC 8058 §3: the one-click headers MUST be covered by the DKIM
        // signature the receiver validates, or the unsubscribe POST is refused.
        "List-Unsubscribe",
        "List-Unsubscribe-Post",
    ]
    .into_iter()
    .map(HeaderName::new_from_ascii_str)
    .collect();

    let config = DkimConfig::new(
        signer.selector.clone(),
        signer.domain.clone(),
        key,
        headers,
        DkimCanonicalization {
            header: DkimCanonicalizationType::Relaxed,
            body: DkimCanonicalizationType::Relaxed,
        },
    );
    dkim_sign(message, &config);
    Ok(())
}

/// Outcome of a transport send. Carries a provider message id when a
/// backend returns one; `None` for SMTP, where the RFC Message-ID is the
/// only identity.
pub struct SendOutcome {
    pub provider_message_id: Option<String>,
}

/// A structured SMTP send failure. Replaces the old stringly-typed error so the
/// queue worker can key retry / suppression decisions on an authoritative SMTP
/// code (from lettre) instead of scraping it out of the error text.
#[derive(Debug, thiserror::Error)]
pub enum SmtpError {
    /// The transport isn't configured (no relay). Never retryable.
    #[error("email is not configured")]
    NotConfigured,
    /// Message build / address / envelope / mailer construction failed before
    /// any network I/O. A local error, never retryable.
    #[error("{0}")]
    Build(String),
    /// The (tenant-controlled) relay host failed SSRF vetting. Config error.
    #[error("relay host rejected: {0}")]
    Egress(#[from] crate::utils::egress::EgressError),
    /// The SMTP send itself failed. `code` is the authoritative status from the
    /// server's reply (absent for transport-level failures — connect, timeout,
    /// TLS); `transient` reflects lettre's own 4xx / timeout classification.
    #[error("smtp send failed: {message}")]
    Smtp {
        code: Option<u16>,
        transient: bool,
        message: String,
    },
}

impl From<String> for SmtpError {
    fn from(message: String) -> Self {
        SmtpError::Build(message)
    }
}

impl SmtpError {
    /// Preserve what lettre already computed: the SMTP status code and whether
    /// the failure is transient, rather than flattening to `Display` text.
    fn from_lettre(e: lettre::transport::smtp::Error) -> Self {
        SmtpError::Smtp {
            code: e.status().map(u16::from),
            transient: e.is_transient() || e.is_timeout(),
            message: e.to_string(),
        }
    }

    /// The authoritative SMTP status code, when the failure was a server reply.
    /// `None` for build / egress / transport-level failures.
    pub fn smtp_code(&self) -> Option<u16> {
        match self {
            SmtpError::Smtp { code, .. } => *code,
            _ => None,
        }
    }

    /// Whether the underlying SMTP reply was a transient (4xx) failure or a
    /// timeout. Build / egress / not-configured are never transient.
    pub fn is_transient(&self) -> bool {
        match self {
            SmtpError::Smtp { transient, .. } => *transient,
            _ => false,
        }
    }
}

/// Email transport seam. SMTP is the only implementation; the trait keeps
/// composition (building the message) in `EmailService` and the provider
/// hand-off behind a swappable boundary, which also makes the send path
/// mockable in tests.
#[async_trait]
pub trait EmailTransport: Send + Sync {
    async fn send(&self, msg: &OutboundEmailMessage<'_>) -> Result<SendOutcome, SmtpError>;
    fn is_configured(&self) -> bool;
    /// Stable identifier for status reporting (currently always `"smtp"`).
    fn provider_name(&self) -> &'static str;
}

/// SMTP transport via lettre. The default provider.
pub struct SmtpEmailTransport {
    config: EmailConfig,
    /// Optional DKIM signer. `Some` for a workspace sending from its verified
    /// domain (signs `d=domain`); `None` leaves signing to the relay or a
    /// later platform-domain signer.
    dkim: Option<DkimSigner>,
    /// True when `config.smtp_host` is tenant-supplied (the `smtp_relay` mode
    /// dials a workspace's own relay). Gates the SSRF resolve-and-validate +
    /// connect-to-validated-address in `send`. The env relay and the
    /// verified-domain relay are operator config and stay `false`.
    untrusted_host: bool,
    /// Per-command SMTP timeout. `None` keeps lettre's default (60s), which the
    /// operator's own relay has always used; a workspace relay gets a shorter
    /// bound so a dead host cannot hold a send for a minute.
    timeout: Option<std::time::Duration>,
}

/// How long a workspace's own relay may take per SMTP command during normal
/// sending.
pub const UNTRUSTED_RELAY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl SmtpEmailTransport {
    pub fn new(config: EmailConfig) -> Self {
        Self {
            config,
            dkim: None,
            untrusted_host: false,
            timeout: None,
        }
    }

    /// Construct with a DKIM signer, so every send is signed `d=domain` before
    /// the relay hands it off.
    pub fn with_dkim(config: EmailConfig, dkim: Option<DkimSigner>) -> Self {
        Self {
            config,
            dkim,
            untrusted_host: false,
            timeout: None,
        }
    }

    /// Construct for a tenant-supplied relay host (`smtp_relay` mode). Every
    /// send SSRF-validates the host and connects to a validated address.
    pub fn new_untrusted(config: EmailConfig, timeout: std::time::Duration) -> Self {
        Self {
            config,
            dkim: None,
            untrusted_host: true,
            timeout: Some(timeout),
        }
    }

    /// Whether this transport can send. Neither relay needs credentials (an
    /// IP-allowlisted relay takes none). A workspace's own relay needs only a
    /// host; the operator's env relay also needs a From address.
    fn can_send(&self) -> bool {
        if self.untrusted_host {
            self.config.enabled && !self.config.smtp_host.trim().is_empty()
        } else {
            self.config.is_configured()
        }
    }
}

#[async_trait]
impl EmailTransport for SmtpEmailTransport {
    async fn send(&self, msg: &OutboundEmailMessage<'_>) -> Result<SendOutcome, SmtpError> {
        if !self.can_send() {
            return Err(SmtpError::NotConfigured);
        }
        let mut message = build_outbound_message(&self.config, msg)?;
        if let Some(signer) = &self.dkim {
            dkim_sign_message(&mut message, signer)?;
        }
        let mailer = if self.untrusted_host {
            // The relay host is tenant-controlled. Resolve + SSRF-vet it and
            // connect to a validated address rather than letting lettre
            // re-resolve the hostname, so a record pointing at an internal /
            // metadata IP (or a rebind between the check and the connect) can't
            // be reached. TLS still validates against the hostname (SNI + cert).
            let addrs = crate::utils::egress::resolve_and_validate(
                &self.config.smtp_host,
                self.config.smtp_port,
            )
            .await?;
            let ip = addrs
                .first()
                .ok_or_else(|| "no validated address for relay host".to_string())?
                .ip();
            // All returned addresses are vetted (resolve_and_validate fails
            // closed if any is non-routable), so the first is safe to use.
            build_smtp_mailer_for(
                &self.config,
                &ip.to_string(),
                &self.config.smtp_host,
                self.timeout,
            )?
        } else {
            build_smtp_mailer_for(
                &self.config,
                &self.config.smtp_host,
                &self.config.smtp_host,
                self.timeout,
            )?
        };
        // B1: when a VERP Return-Path is set, send with an explicit envelope so
        // `MAIL FROM` is the bounce-token address, distinct from the `From`
        // header. lettre's default `send` derives the envelope from `From`;
        // overriding it needs `send_raw` with the formatted (DKIM-signed) bytes.
        let envelope = match msg.envelope_from {
            Some(return_path) => {
                let from: lettre::Address = return_path
                    .parse()
                    .map_err(|e| format!("Invalid Return-Path {return_path}: {e}"))?;
                let to: lettre::Address = msg
                    .to
                    .parse()
                    .map_err(|e| format!("Invalid recipient {}: {e}", msg.to))?;
                Some(
                    lettre::address::Envelope::new(Some(from), vec![to])
                        .map_err(|e| format!("Invalid envelope: {e}"))?,
                )
            }
            None => None,
        };
        // lettre's SmtpTransport is synchronous: connecting, TLS and every SMTP
        // round trip block the calling thread, for up to the timeout. Run it on
        // the blocking pool so a slow or dead relay never stalls an async
        // worker (the queue worker and the admin test send both land here).
        tokio::task::spawn_blocking(move || match envelope {
            Some(envelope) => mailer.send_raw(&envelope, &message.formatted()),
            None => mailer.send(&message),
        })
        .await
        .map_err(|e| SmtpError::Build(format!("send task failed: {e}")))?
        .map_err(SmtpError::from_lettre)?;
        Ok(SendOutcome {
            provider_message_id: None,
        })
    }

    fn is_configured(&self) -> bool {
        self.can_send()
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
    /// A workspace's own SMTP server (`smtp_relay` mode) rather than the
    /// instance relay. The queue gives each one its own circuit breaker.
    tenant_relay: bool,
}

impl EmailService {
    /// Create a new email service with the given configuration
    pub fn new(config: EmailConfig) -> Self {
        // SMTP is the only transport and the self-host standard.
        let transport: Arc<dyn EmailTransport> = Arc::new(SmtpEmailTransport::new(config.clone()));
        Self {
            config,
            transport,
            tenant_relay: false,
        }
    }

    /// Create an SMTP email service that DKIM-signs every send with `dkim`.
    /// Used for the verified-domain mode: the workspace sends from its own
    /// `From` through the instance relay, signed `d=domain`.
    pub fn smtp_with_dkim(config: EmailConfig, dkim: Option<DkimSigner>) -> Self {
        let transport: Arc<dyn EmailTransport> =
            Arc::new(SmtpEmailTransport::with_dkim(config.clone(), dkim));
        Self {
            config,
            transport,
            tenant_relay: false,
        }
    }

    /// Create a service for a tenant-supplied SMTP relay (`smtp_relay` mode).
    /// The relay host is SSRF-validated on every send and the connection goes to
    /// a validated address (see `SmtpEmailTransport::new_untrusted`).
    pub fn new_untrusted_relay(config: EmailConfig) -> Self {
        Self::new_untrusted_relay_with_timeout(config, UNTRUSTED_RELAY_TIMEOUT)
    }

    /// [`Self::new_untrusted_relay`] with an explicit per-command timeout; the
    /// admin test send uses a short one so the form answers promptly.
    pub fn new_untrusted_relay_with_timeout(
        config: EmailConfig,
        timeout: std::time::Duration,
    ) -> Self {
        let transport: Arc<dyn EmailTransport> =
            Arc::new(SmtpEmailTransport::new_untrusted(config.clone(), timeout));
        Self {
            config,
            transport,
            tenant_relay: true,
        }
    }

    /// True for a workspace's own SMTP server, false for the instance relay.
    pub fn is_tenant_relay(&self) -> bool {
        self.tenant_relay
    }

    /// Create the email service from environment variables. SMTP is the only
    /// transport: every provider (SES, Mailgun, ...) offers SMTP submission,
    /// and per-workspace DKIM signing rides on the SMTP path.
    pub fn from_env() -> Result<Self, String> {
        let config = EmailConfig::from_env()?;
        Ok(Self::new(config))
    }

    /// True when the active transport can send (on, with a host and a From
    /// address; credentials are optional). Delegates to the transport rather than the raw SMTP-centric
    /// `EmailConfig::is_configured`.
    pub fn is_configured(&self) -> bool {
        self.transport.is_configured()
    }

    /// Active provider identifier for admin status reporting
    /// (currently always `"smtp"`).
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

    /// Send a simple text email through the configured SMTP transport, so
    /// direct sends share the same path as the queue.
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
            // Generic direct-send path; its callers (test mail, guest ticket
            // confirmation) are transactional, the safe no-unsubscribe default.
            mail_class: crate::models::outbound_email_mail_class::TRANSACTIONAL,
            reply_to: None,
            envelope_from: None,
            list_unsubscribe: None,
        };
        self.send_outbound(&outbound)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
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
            // Generic direct-send path; its callers (test mail, guest ticket
            // confirmation) are transactional, the safe no-unsubscribe default.
            mail_class: crate::models::outbound_email_mail_class::TRANSACTIONAL,
            reply_to: None,
            envelope_from: None,
            list_unsubscribe: None,
        };
        self.send_outbound(&outbound)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Send a test email. The typed error lets the admin test say which step
    /// failed (DNS, blocked host, auth, ...).
    /// Send the test letter: the workspace's letterhead and colours, and the
    /// server it went through.
    pub async fn send_test(
        &self,
        to: &str,
        branding: &EmailBranding,
        locale: &unic_langid::LanguageIdentifier,
    ) -> Result<(), SmtpError> {
        let server = format!("{}:{}", self.config.smtp_host, self.config.smtp_port);
        let from = format!("{} <{}>", self.config.from_name, self.config.from_email);
        let (subject, body_html, body_text) = test_letter(
            &EmailTemplate::new(branding),
            locale,
            Some((&server, &from)),
        );
        let message_id = self.generate_message_id();
        let outbound = OutboundEmailMessage {
            to,
            subject: &subject,
            body_text: &body_text,
            body_html: Some(&body_html),
            message_id: &message_id,
            in_reply_to: None,
            references: &[],
            auto_submitted: None,
            mail_class: crate::models::outbound_email_mail_class::TRANSACTIONAL,
            reply_to: None,
            envelope_from: None,
            list_unsubscribe: None,
        };
        self.send_outbound(&outbound).await.map(|_| ())
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

        (subject, html_body, template.finish_text(body_text))
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

        (subject, html_body, template.finish_text(body_text))
    }

    /// Render the customer-portal passwordless sign-in email. Returns
    /// `(subject, html_body, body_text)`. Same HTML/plaintext split as the
    /// invitation; the CTA links to the portal callback on the workspace's own
    /// origin (carried in `branding.base_url`).
    /// "You've been added to a request": sent when a requester adds a
    /// colleague to their request. Links in signed in, like View request.
    pub fn compose_participant_added(
        &self,
        adder_name: &str,
        ticket_number: i32,
        ticket_title: &str,
        view_url: &str,
        branding: &EmailBranding,
        locale: &unic_langid::LanguageIdentifier,
    ) -> (String, String, String) {
        let template = EmailTemplate::new(branding);
        let tr = |key: &str, args: &[(&str, fluent_bundle::FluentValue<'static>)]| {
            crate::utils::i18n::tr_with(locale, key, args)
        };
        let args = |escape: bool| -> Vec<(&'static str, fluent_bundle::FluentValue<'static>)> {
            let e = |v: &str| {
                if escape {
                    escape_html(v)
                } else {
                    v.to_string()
                }
            };
            vec![
                ("adder", e(adder_name).into()),
                ("id", ticket_number.to_string().into()),
                ("title", e(ticket_title).into()),
            ]
        };
        let headline = tr("participant-added-title", &[]);
        let html_body = template.render(
            EmailLayout {
                headline: &headline,
                body: vec![
                    text(tr("participant-added-body", &args(true))),
                    text(tr("participant-added-replies", &[])),
                ],
                cta: Some(Cta {
                    label: tr("participant-added-cta", &[]),
                    url: view_url.to_string(),
                }),
                notice: None,
                signoff: None,
                preheader: &headline,
            },
            locale,
        );
        let mut subject_args = args(false);
        subject_args.push(("app", branding.app_name.clone().into()));
        let subject = tr("participant-added-subject", &subject_args);
        let body_text = format!(
            "{}\n\n{}\n\n{}: {view_url}\n",
            tr("participant-added-body", &args(false)),
            tr("participant-added-replies", &[]),
            tr("participant-added-cta", &[]),
        );
        (subject, html_body, template.finish_text(body_text))
    }

    pub fn compose_portal_magic_link(
        &self,
        user_name: &str,
        magic_token: &str,
        code: Option<&str>,
        branding: &EmailBranding,
        locale: &unic_langid::LanguageIdentifier,
    ) -> (String, String, String) {
        let sign_in_link = format!(
            "{}/api/portal/auth/callback?token={}",
            branding.base_url, magic_token
        );
        let template = EmailTemplate::new(branding);
        let tr = |key: &str, args: &[(&str, fluent_bundle::FluentValue<'static>)]| {
            crate::utils::i18n::tr_with(locale, key, args)
        };

        let name_html = escape_html(user_name);
        let app_html = escape_html(&branding.app_name);

        let title = tr(
            "portal-magic-link-title",
            &[("app", app_html.clone().into())],
        );
        let greeting = tr(
            "portal-magic-link-greeting",
            &[("name", name_html.clone().into())],
        );
        let intro = tr(
            "portal-magic-link-intro",
            &[("app", app_html.clone().into())],
        );
        let cta_label = tr("portal-magic-link-cta-label", &[]);
        // The same sign-in as a code, for when the email is open on another
        // device. Shown as "123 456" so it reads at a glance.
        let code_display = code.map(|c| {
            if c.len() == 6 {
                format!("{} {}", &c[..3], &c[3..])
            } else {
                c.to_string()
            }
        });
        let mut notice_items: Vec<String> = Vec::new();
        if let Some(code) = &code_display {
            notice_items.push(tr(
                "portal-magic-link-code",
                &[("code", escape_html(code).into())],
            ));
        }
        notice_items.extend(
            [
                "portal-magic-link-notice-expiry",
                "portal-magic-link-notice-unexpected",
            ]
            .iter()
            .map(|key| tr(key, &[])),
        );

        let html_body = template.render(
            EmailLayout {
                headline: &title,
                body: vec![text(greeting), text(intro)],
                cta: Some(Cta {
                    label: cta_label,
                    url: sign_in_link.clone(),
                }),
                notice: Some(Notice {
                    kind: NoticeType::Info,
                    items: notice_items,
                }),
                signoff: None,
                preheader: &title,
            },
            locale,
        );

        let subject = tr(
            "portal-magic-link-subject",
            &[("app", branding.app_name.clone().into())],
        );

        let mut body_text = tr(
            "portal-magic-link-body-text",
            &[
                ("name", user_name.to_string().into()),
                ("app", branding.app_name.clone().into()),
                ("link", sign_in_link.clone().into()),
            ],
        );
        if let Some(code) = &code_display {
            let line = tr(
                "portal-magic-link-code-text",
                &[("code", code.clone().into())],
            );
            body_text = format!("{body_text}\n\n{line}");
        }

        (subject, html_body, template.finish_text(body_text))
    }

    /// Compose the "confirm this address" email for an address added to a
    /// profile.
    ///
    /// Deliberately shaped like [`Self::compose_portal_magic_link`]: the same
    /// proof (possession of a link sent to the address) with a much smaller
    /// consequence, since clicking it verifies one address rather than signing
    /// anyone in. The copy says which address is being confirmed, because a
    /// user with several addresses cannot otherwise tell which one this is
    /// about.
    pub fn compose_email_verification(
        &self,
        user_name: &str,
        address: &str,
        verification_token: &str,
        branding: &EmailBranding,
        locale: &unic_langid::LanguageIdentifier,
    ) -> (String, String, String) {
        let verify_link = format!(
            "{}/verify-email?token={}",
            branding.base_url, verification_token
        );
        let template = EmailTemplate::new(branding);
        let tr = |key: &str, args: &[(&str, fluent_bundle::FluentValue<'static>)]| {
            crate::utils::i18n::tr_with(locale, key, args)
        };

        let name_html = escape_html(user_name);
        let app_html = escape_html(&branding.app_name);
        let address_html = escape_html(address);

        let title = tr("email-verify-title", &[("app", app_html.clone().into())]);
        let greeting = tr(
            "email-verify-greeting",
            &[("name", name_html.clone().into())],
        );
        let intro = tr(
            "email-verify-intro",
            &[
                ("app", app_html.clone().into()),
                ("address", address_html.clone().into()),
            ],
        );
        let cta_label = tr("email-verify-cta-label", &[]);
        let notice_items: Vec<String> = [
            "email-verify-notice-expiry",
            "email-verify-notice-unexpected",
        ]
        .iter()
        .map(|key| tr(key, &[]))
        .collect();

        let html_body = template.render(
            EmailLayout {
                headline: &title,
                body: vec![text(greeting), text(intro)],
                cta: Some(Cta {
                    label: cta_label,
                    url: verify_link.clone(),
                }),
                notice: Some(Notice {
                    kind: NoticeType::Info,
                    items: notice_items,
                }),
                signoff: None,
                preheader: &title,
            },
            locale,
        );

        let subject = tr(
            "email-verify-subject",
            &[("app", branding.app_name.clone().into())],
        );

        let body_text = tr(
            "email-verify-body-text",
            &[
                ("name", user_name.to_string().into()),
                ("address", address.to_string().into()),
                ("link", verify_link.clone().into()),
            ],
        );

        (subject, html_body, template.finish_text(body_text))
    }

    /// Render the guest ticket-confirmation email without sending. Returns
    /// `(subject, html_body)`; the plain-text alternative is derived from the
    /// HTML by `send_html_email`. Split out so the preview harness can render
    /// it without a transport.
    pub fn compose_guest_ticket_confirmation(
        &self,
        invitation_token: &str,
        branding: &EmailBranding,
        stop_url: Option<&str>,
    ) -> (String, String, String) {
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
        // Nothing the submitter typed goes in this email: it reaches whatever
        // address was entered, so it says only what the workspace says.
        let greeting = "Hi,".to_string();
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
                    note("If you didn't submit a ticket, you can safely ignore this email."),
                ]
                .into_iter()
                .chain(stop_url.map(|url| {
                    muted(format!(
                        r#"Not you? <a href="{}" style="color:{C_LINK};">Stop these emails</a>."#,
                        escape_html(url)
                    ))
                }))
                .collect(),
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
        let mut body_text = format!(
            "Thanks for submitting a ticket to {app}. Confirm your email to release it to our team:\n\n{confirm_link}\n\nThe link expires in 7 days. If you didn't submit a ticket, you can safely ignore this email.",
            app = branding.app_name,
        );
        if let Some(url) = stop_url {
            body_text.push_str(&format!("\n\nNot you? Stop these emails: {url}"));
        }
        (subject, html_body, template.finish_text(body_text))
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
        self.send_outbound(&outbound)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Send and return the transport outcome. SMTP carries no provider
    /// message id (`None`); the field exists for a future transport that
    /// returns one. `send_ticket_reply` is the thin wrapper for callers
    /// that don't need the outcome (auto-ack, IMAP).
    pub async fn send_outbound(
        &self,
        outbound: &OutboundEmailMessage<'_>,
    ) -> Result<SendOutcome, SmtpError> {
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
        cta_label: Option<&str>,
        feedback: Option<&FeedbackLinks>,
        branding: &EmailBranding,
        locale: &unic_langid::LanguageIdentifier,
    ) -> (String, String) {
        let template = EmailTemplate::new(branding);
        let tr = |key: &str, args: &[(&str, fluent_bundle::FluentValue<'static>)]| {
            crate::utils::i18n::tr_with(locale, key, args)
        };

        // No "From" row when there's no separate actor (an acknowledgement of
        // the recipient's own request).
        let from_row = (!actor_name.is_empty()).then(|| {
            tr(
                "notif-from-row",
                &[("actor", escape_html(actor_name).into())],
            )
        });

        let button_label = cta_label.map(str::to_string).unwrap_or_else(|| {
            tr(
                "notif-cta-view-in",
                &[("app", branding.app_name.clone().into())],
            )
        });
        let footer = tr("notif-footer-preferences", &[]);
        // Two equal links, neither styled as the primary action.
        let feedback_html = feedback.map(|f| {
            let link = |url: &str, label: String| {
                format!(
                    r#"<a class="nd-link" href="{url}" style="color:{C_LINK};">{label}</a>"#,
                    url = escape_html(url),
                    label = escape_html(&label),
                )
            };
            format!(
                "{} {} &nbsp;&middot;&nbsp; {}",
                escape_html(&tr("notif-feedback-question", &[])),
                link(&f.fixed, tr("notif-feedback-fixed", &[])),
                link(&f.not_fixed, tr("notif-feedback-not-fixed", &[])),
            )
        });
        let text_body = match feedback {
            Some(f) => format!(
                "{body}\n\n{}\n{}: {}\n{}: {}",
                tr("notif-feedback-question", &[]),
                tr("notif-feedback-fixed", &[]),
                f.fixed,
                tr("notif-feedback-not-fixed", &[]),
                f.not_fixed,
            ),
            None => body.to_string(),
        };
        let html_body = template.render(
            EmailLayout {
                headline: title,
                // Blank lines separate paragraphs (an acknowledgement's
                // known-issue note, for one).
                body: body
                    .split("\n\n")
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(|p| text(escape_html(p)))
                    .chain(feedback_html.map(text))
                    .chain(from_row.map(muted))
                    .collect(),
                cta: Some(Cta {
                    label: button_label.clone(),
                    url: cta_url.to_string(),
                }),
                notice: None,
                signoff: None,
                preheader: &footer,
                ..Default::default()
            },
            locale,
        );

        let body_text = if actor_name.is_empty() {
            tr(
                "notif-body-text-no-actor",
                &[
                    ("title", title.to_string().into()),
                    ("body", text_body.clone().into()),
                    ("app", branding.app_name.clone().into()),
                    ("cta", cta_url.to_string().into()),
                    ("cta_label", button_label.clone().into()),
                ],
            )
        } else {
            tr(
                "notif-body-text",
                &[
                    ("title", title.to_string().into()),
                    ("body", text_body.clone().into()),
                    ("actor", actor_name.to_string().into()),
                    ("app", branding.app_name.clone().into()),
                    ("cta", cta_url.to_string().into()),
                    ("cta_label", button_label.clone().into()),
                ],
            )
        };

        (html_body, template.finish_text(body_text))
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
    /// Mail class (see `models::outbound_email_mail_class`): `"notification"`
    /// (opt-out-able) or `"transactional"` (must-deliver). Carried to the send
    /// path so deliverability headers branch on it (List-Unsubscribe on
    /// notification only). Defaults to transactional on any path that hasn't
    /// classified itself, which is the safe, no-unsubscribe choice.
    pub mail_class: &'a str,
    /// `Reply-To` address (B3). Set on channel-bound conversation mail to the
    /// channel's polled inbound mailbox so a recipient's reply threads back into
    /// the ticket even when the `From` is a different workspace send identity
    /// (verified-domain / relay mode). `None` emits no header, leaving the
    /// `From` as the implicit reply target.
    pub reply_to: Option<&'a str>,
    /// VERP envelope-from / Return-Path (B1). When `Some`, the message is sent
    /// with this `MAIL FROM` (distinct from the `From` header) so a bounce DSN
    /// is addressed back to it and the inbound handler can link the bounce to
    /// the originating row by its token. `None` (the default, and whenever
    /// `SMTP_VERP_SECRET` is unset) uses lettre's From-derived envelope.
    pub envelope_from: Option<&'a str>,
    /// `List-Unsubscribe` URL (B2 / RFC 8058). Set on opt-out-able notification
    /// mail to a signed one-click endpoint; emits `List-Unsubscribe` plus
    /// `List-Unsubscribe-Post: List-Unsubscribe=One-Click`. `None` on
    /// transactional mail (which must not advertise unsubscribe).
    pub list_unsubscribe: Option<&'a str>,
}

/// The test letter on one paper, as the Branding page previews it.
pub fn preview_test_letter(
    branding: &EmailBranding,
    locale: &unic_langid::LanguageIdentifier,
    paper: Paper,
) -> String {
    test_letter(&EmailTemplate::preview(branding, paper), locale, None).1
}

/// What the workspace's mail looks like, and when it was really sent, the
/// server it went through (`server`, `from`). Returns
/// `(subject, body_html, body_text)`.
fn test_letter(
    template: &EmailTemplate<'_>,
    locale: &unic_langid::LanguageIdentifier,
    sent_through: Option<(&str, &str)>,
) -> (String, String, String) {
    let app = template.branding.app_name.as_str();
    let tr = |key: &str, args: &[(&str, fluent_bundle::FluentValue<'static>)]| {
        crate::utils::i18n::tr_with(locale, key, args)
    };
    let app_arg = |escape: bool| -> Vec<(&'static str, fluent_bundle::FluentValue<'static>)> {
        let value = if escape {
            escape_html(app)
        } else {
            app.to_string()
        };
        vec![("app", value.into())]
    };

    let headline = tr("email-test-message-headline", &app_arg(false));
    let mut body = vec![text(tr("email-test-message-body", &app_arg(true)))];
    let mut plain = vec![tr("email-test-message-body", &app_arg(false))];
    if let Some((server, from)) = sent_through {
        let details = |escape: bool| -> Vec<(&'static str, fluent_bundle::FluentValue<'static>)> {
            let e = |v: &str| {
                if escape {
                    escape_html(v)
                } else {
                    v.to_string()
                }
            };
            vec![("server", e(server).into()), ("from", e(from).into())]
        };
        body.push(text(tr("email-test-message-delivered", &[])));
        body.push(muted(tr("email-test-message-server", &details(true))));
        plain.push(tr("email-test-message-delivered", &[]));
        plain.push(tr("email-test-message-server", &details(false)));
    }
    let label = tr("email-test-message-cta", &app_arg(false));
    let url = template.branding.base_url.clone();
    plain.push(format!("{label}: {url}"));

    let html = template.render(
        EmailLayout {
            headline: &headline,
            body,
            cta: Some(Cta { label, url }),
            ..Default::default()
        },
        locale,
    );
    let subject = tr("email-test-message-subject", &app_arg(false));
    (
        subject,
        html,
        template.finish_text(plain.join("\n\n")) + "\n",
    )
}

/// `scheme://host[:port]` of a URL, with any path dropped. A base without a
/// scheme is returned unchanged.
fn origin_of(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    match base.find("://") {
        Some(i) => match base[i + 3..].find('/') {
            Some(j) => base[..i + 3 + j].to_string(),
            None => base.to_string(),
        },
        None => base.to_string(),
    }
}

#[cfg(test)]
mod tests {
    fn copy(url: &str, (width, height): (u32, u32), light: bool, dark: bool) -> super::EmailLogo {
        super::EmailLogo {
            url: url.into(),
            width,
            height,
            reads_on_light: light,
            reads_on_dark: dark,
        }
    }

    fn letterhead(branding: &super::EmailBranding) -> String {
        super::EmailTemplate::new(branding).build_logo_section()
    }

    #[test]
    fn logo_resolves_against_the_origin_not_the_slug_base() {
        let mut branding = super::EmailBranding::default();
        branding.logo = Some(copy(
            "/uploads/branding/ws/email_logo_1.png",
            (200, 40),
            true,
            true,
        ));
        branding.base_url = "https://app.nosdesk.example/acme".into();
        let html = letterhead(&branding);
        assert!(
            html.contains(
                r#"src="https://app.nosdesk.example/uploads/branding/ws/email_logo_1.png""#
            ),
            "{html}"
        );

        branding.base_url = "https://acme.nosdesk.example/".into();
        let html = letterhead(&branding);
        assert!(html.contains(
            r#"src="https://acme.nosdesk.example/uploads/branding/ws/email_logo_1.png""#
        ));

        branding.logo = Some(copy("https://cdn.example/logo.png", (200, 40), true, true));
        branding.base_url = "https://app.nosdesk.example/acme".into();
        assert!(letterhead(&branding).contains(r#"src="https://cdn.example/logo.png""#));
    }

    #[test]
    fn the_logo_is_drawn_at_its_copys_size() {
        let mut branding = super::EmailBranding::default();
        branding.logo = Some(copy("/l.png", (48, 48), true, true));
        let html = letterhead(&branding);
        assert!(html.contains(r#"width="48" height="48""#), "{html}");
        assert!(html.contains("width:48px;height:48px;"), "{html}");
        assert!(!html.contains("150"), "no fixed size left: {html}");
    }

    #[test]
    fn a_logo_that_reads_on_both_papers_appears_once() {
        let mut branding = super::EmailBranding::default();
        branding.logo = Some(copy("/l.png", (200, 40), true, true));
        let html = letterhead(&branding);
        assert_eq!(html.matches("<img").count(), 1, "{html}");
        assert!(!html.contains("nd-dark-only"), "{html}");
    }

    #[test]
    fn the_light_theme_logo_leads_on_light_paper_and_the_main_one_on_dark() {
        let mut branding = super::EmailBranding::default();
        branding.logo = Some(copy("/main.png", (200, 40), false, true));
        branding.logo_light = Some(copy("/light.png", (200, 40), true, false));
        let html = letterhead(&branding);
        let light = html.find("/light.png").expect("light-theme logo");
        let main = html.find("/main.png").expect("main logo");
        assert!(light < main, "the light paper's logo comes first: {html}");
        assert!(html.contains(r#"<div class="nd-light-only">"#), "{html}");
        assert!(
            html.contains(r#"class="nd-dark-only" style="display:none;"#),
            "the dark paper's logo is hidden unless the client switches: {html}"
        );
        assert!(
            html.contains("mso-hide:all"),
            "desktop Outlook never shows it: {html}"
        );
        assert!(
            !html.contains("background-color"),
            "each reads bare on its paper: {html}"
        );
    }

    #[test]
    fn a_dark_logo_sits_on_light_paper_in_dark_mode() {
        let mut branding = super::EmailBranding::default();
        branding.logo = Some(copy("/navy.png", (200, 40), true, false));
        let html = letterhead(&branding);
        let (light, dark) = html.split_once("nd-dark-only").expect("a dark block");
        assert!(
            !light.contains("background-color"),
            "bare on the light paper: {html}"
        );
        assert!(
            dark.contains("background-color:#f6f2ea"),
            "backed in dark mode: {html}"
        );
    }

    #[test]
    fn a_white_logo_sits_on_dark_paper_in_light_mode() {
        let mut branding = super::EmailBranding::default();
        branding.logo = Some(copy("/white.png", (200, 40), false, true));
        let html = letterhead(&branding);
        let (light, dark) = html.split_once("nd-dark-only").expect("a dark block");
        assert!(
            light.contains("background-color:#0b0a08"),
            "backed on light paper: {html}"
        );
        assert!(
            !dark.contains("background-color"),
            "bare in dark mode: {html}"
        );
    }

    #[test]
    fn a_logo_that_reads_on_neither_paper_is_shown_bare() {
        let mut branding = super::EmailBranding::default();
        branding.logo = Some(copy("/grey.png", (200, 40), false, false));
        assert!(!letterhead(&branding).contains("background-color"));
    }

    #[test]
    fn no_logo_and_the_nosdesk_name_sign_with_the_wordmark() {
        let mut branding = super::EmailBranding::default();
        branding.base_url = "https://app.nosdesk.example/acme".into();
        let html = letterhead(&branding);
        assert!(
            html.contains(
                r#"src="https://app.nosdesk.example/email-assets/nosdesk-wordmark.png?v=1" width="148" height="27""#
            ),
            "served by the instance itself: {html}"
        );
        assert!(html.contains(r#"alt="Nosdesk""#), "{html}");
    }

    #[test]
    fn no_logo_and_a_custom_name_set_the_name_in_its_colour() {
        let mut branding = super::EmailBranding::default();
        branding.app_name = "Acme IT".into();
        branding.primary_color = "#1e3a8a".into();
        let template = super::EmailTemplate::new(&branding);
        let html = template.build_logo_section();
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains(r#"class="nd-mark""#), "{html}");
        assert!(
            html.contains("color:#1e3a8a;"),
            "navy reads on light paper: {html}"
        );
        assert_eq!(
            template.mark_colors().1,
            "#f4f1ea",
            "navy is lost on the dark paper, so the name turns light there"
        );

        branding.primary_color = "#ffd600".into();
        let template = super::EmailTemplate::new(&branding);
        assert_eq!(
            template.mark_colors(),
            (super::C_HEAD.to_string(), "#ffd600".to_string()),
            "yellow is lost on the light paper"
        );
    }

    #[test]
    fn the_button_takes_the_brand_colour_with_text_that_reads_on_it() {
        let locale: unic_langid::LanguageIdentifier = "en-US".parse().unwrap();
        let cta = super::Cta {
            label: "Open".into(),
            url: "https://desk.example/x".into(),
        };
        for (brand, text) in [
            ("#ffd600", "#000000"),
            ("#1e3a8a", "#ffffff"),
            ("#FF6B1A", "#000000"),
        ] {
            let mut branding = super::EmailBranding::default();
            branding.primary_color = brand.into();
            let html = super::EmailTemplate::new(&branding).build_cta_section(&cta, &locale);
            let fill = brand.to_lowercase();
            assert!(
                html.contains(&format!("background-color:{fill};color:{text};")),
                "{brand}: {html}"
            );
            assert!(
                html.contains(&format!(r#"strokecolor="{fill}" fillcolor="{fill}""#)),
                "Outlook's button too: {html}"
            );
            assert!(
                html.contains(&format!("<center style=\"color:{text};")),
                "Outlook's label too: {html}"
            );
        }
    }

    #[test]
    fn an_unreadable_brand_colour_falls_back_to_nosdesk_orange() {
        let mut branding = super::EmailBranding::default();
        branding.primary_color = "blue".into();
        assert_eq!(branding.brand(), super::NOSDESK_ORANGE);
    }

    #[test]
    fn mail_switches_paper_with_the_readers_setting() {
        let html = super::EmailTemplate::new(&super::EmailBranding::default()).render(
            super::EmailLayout {
                headline: "Hi",
                ..Default::default()
            },
            &"en-US".parse().unwrap(),
        );
        assert!(
            html.contains("@media (prefers-color-scheme: dark)"),
            "{html}"
        );
        assert!(html.contains(r#"content="light dark""#));
        assert!(!html.contains("<base"), "mail links open normally");
    }

    #[test]
    fn a_preview_is_drawn_on_one_paper_with_images_from_the_page() {
        let locale: unic_langid::LanguageIdentifier = "en-US".parse().unwrap();
        let mut branding = super::EmailBranding::default();
        branding.base_url = "https://app.nosdesk.example/acme".into();
        branding.logo = Some(copy(
            "/uploads/branding/ws/email_logo_1.png",
            (200, 40),
            true,
            false,
        ));

        let light = super::preview_test_letter(&branding, &locale, super::Paper::Light);
        assert!(!light.contains("prefers-color-scheme"), "{light}");
        assert!(
            !light.contains(".nd-paper"),
            "no dark rules at all: {light}"
        );
        assert!(
            light.contains(r#"src="/uploads/branding/ws/email_logo_1.png""#),
            "{light}"
        );
        assert!(light.contains(r#"<base target="_blank">"#));

        let dark = super::preview_test_letter(&branding, &locale, super::Paper::Dark);
        assert!(!dark.contains("prefers-color-scheme"), "{dark}");
        assert!(
            dark.contains(".nd-paper   { background:#0b0a08 !important; }"),
            "{dark}"
        );
        assert!(
            dark.contains(".nd-dark-only  { display:block !important;"),
            "{dark}"
        );
        assert!(dark.contains(r#"content="dark""#));
    }

    #[test]
    fn the_test_letter_names_the_server_it_went_through() {
        let locale: unic_langid::LanguageIdentifier = "en-US".parse().unwrap();
        let mut branding = super::EmailBranding::default();
        branding.app_name = "Acme <IT>".into();
        let (subject, html, text) = super::test_letter(
            &super::EmailTemplate::new(&branding),
            &locale,
            Some(("smtp.example.com:587", "Acme <help@acme.test>")),
        );
        assert!(subject.contains("Acme <IT>"), "{subject}");
        assert!(html.contains("smtp.example.com:587"), "{html}");
        assert!(
            html.contains("Acme &lt;help@acme.test&gt;"),
            "escaped: {html}"
        );
        assert!(!html.contains("Acme <IT>"), "the name is escaped: {html}");
        assert!(text.contains("smtp.example.com:587"), "{text}");
        assert!(text.contains("Acme <help@acme.test>"), "{text}");

        let preview = super::preview_test_letter(&branding, &locale, super::Paper::Light);
        assert!(
            !preview.contains("smtp.example.com"),
            "a preview was never sent"
        );
    }

    use super::*;

    #[test]
    fn smtp_coherence_matrix() {
        use SmtpSecurity::*;
        // The three protocol-impossible pairs are hard errors.
        assert!(matches!(
            check_port_security(465, StartTls),
            SmtpCoherence::Error(_)
        ));
        assert!(matches!(
            check_port_security(465, Plaintext),
            SmtpCoherence::Error(_)
        ));
        assert!(matches!(
            check_port_security(587, Tls),
            SmtpCoherence::Error(_)
        ));
        // The standard coherent pairs are clean.
        assert_eq!(check_port_security(465, Tls), SmtpCoherence::Ok);
        assert_eq!(check_port_security(587, StartTls), SmtpCoherence::Ok);
        // Insecure-but-reachable and relay-port configs warn, not error.
        assert!(matches!(
            check_port_security(587, Plaintext),
            SmtpCoherence::Warn(_)
        ));
        assert!(matches!(
            check_port_security(25, StartTls),
            SmtpCoherence::Warn(_)
        ));
        // Non-standard ports trust the operator's explicit TLS mode.
        assert_eq!(check_port_security(2525, StartTls), SmtpCoherence::Ok);
        assert_eq!(check_port_security(10025, Tls), SmtpCoherence::Ok);
        // ...but still flag plaintext on any port.
        assert!(matches!(
            check_port_security(2525, Plaintext),
            SmtpCoherence::Warn(_)
        ));
    }

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
                mail_class: "transactional",
                reply_to: None,
                envelope_from: None,
                list_unsubscribe: None,
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
                mail_class: "transactional",
                reply_to: None,
                envelope_from: None,
                list_unsubscribe: None,
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
                        mail_class: "transactional",
                        reply_to: None,
                        envelope_from: None,
                        list_unsubscribe: None,
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

    // ---------- DKIM signing ----------

    const TEST_DKIM_PKCS1_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEAx/pqSWX4u310FOxAxq/1j/qVn3XfZ1aMvKj7YPaFsHsvVpoC
MEW7yCeuX+DqB0aT2hwGrUVJgJVaQ8mtsUFfDtYMxdGSILoBEL1Mfp8v1hfzXUD4
+k3tZPAsaX9fEz0YdXTM+/hkg1e0cXuMZb54Wt4H/vwRxbBmlx+uw0KT6aa1RF7R
ZiW44dqa+4T1lkKg4fX3K/Joa5DzSvng8RhTLkXF4pPRe37tjkA5PanFr+lmrGCM
JF0+R6OP81sg+yYBhhcMl4bQAx1YhWhtkFeBxiMP7COrQooETOKSfjyszT1jF7BD
iT+BzBZF0QQAWMhRwmW8bcv+gK5kng4dkgVbxQIDAQABAoIBACzxNb7OGHLGdHaZ
S8t7UwQrDEI8gtseA94IWgpGDPCHFrHvRaukmFmYtWMd0GqXLXY4kzWQmz63EgSn
CA6Mgvj6GP/CJAWP19pzuIPCccU7N7nO9sWGCuKC6XBCLFNOCTeoasL75VbxOH/C
hOB+yFyfhouDCdl0VfIDsEp4pXY+V84eHxS9ZO///RYRxYMZyC4DgpR/WXtFOHBS
yRMXT4h2rdGkrDg2Wv/OhCylxjatkhSI6P+wbq9fHAHmlzbEF/0XAYm5B6Ar8mzi
+wxiU/S6JVrOIPCxPwkOeqzoHt56rplk4rBxz8woQfTQl+kwIHLWqCtUjLXwogUz
kMdqxFECgYEA40/aaGKCOIWpLunzcKJ32saganor11qVA8VLBEiGbX9vHhPUwZZt
JmTmbPRnVSHPudtR6hJjsm143BLs9w7fHZNVxvyIdAqhoQdHECyuL66q1Lq4jHZi
pU/QLqYDlCI5I3Q4GHp6r0kYwIOVu7B/fR/jozQJdI/fqMW6NnBrSy0CgYEA4Tdx
8Gzmaoxo9CvwM89hxMxMWm+17uNqZg2Q31HorgGI7iTcqi5+zF1JJaYsB2huHhI2
BLl86oXPdjdpxeM3RxIIW+tEjl9woDagHDHL9KPtCCvSa+4R8dZu2rVZ32UgKVq8
3b+vHhp1WphuFygxTXzDUBtxdFpx5q8dgi5gUfkCgYBswNiy1maNGk2+V0oUWnbT
YfJ/3uG4z+q5ehwQ+Y3vN2f3UO+aixi/pMil2izSCzIyLp87SP8P79ZCHH/pF+Fh
agtA/7NdKXT48N1r/KR9xaiPzKHc+grqIoxstRrDNbh2oPTxqS+nS2afPJVXzfLA
74/ellfrv6X3PlqADzsWJQKBgFXWHfT2bHNbhHzbajc06RxqiQdG4F5mCp1OulKD
E12OdDPflMK/6c/WFhTlWo6QPLf1VOVEFNoFmeaChCvJx72sn8b4yi5BLdnCOA/G
4ucguyyMFyzPlcNIaQOubsx37GQWkzko34NnriaTRhJJXVEdJguYCgvAlPzI7UQ6
jLdxAoGBAKZD/KJTS/sYWXHjztl0EmZzYexrS38I+AxdktPA3GqdsA9D2bNJelry
JmP3rOGzew+YvyVrjwfHjkFEusxZQo8yLlv6KMtOEGJgFrVt95ykUr6py3R3t9+k
B88KQSZwPfTv4qlBKPZXpb3vrKIOynaKzM7b7aZYs3LPZwTUb1yq
-----END RSA PRIVATE KEY-----";

    fn dkim_test_config() -> EmailConfig {
        EmailConfig {
            smtp_host: "smtp.example.org".into(),
            smtp_port: 587,
            smtp_username: "u".into(),
            smtp_password: "p".into(),
            from_name: "Support".into(),
            from_email: "support@example.org".into(),
            enabled: true,
            security: SmtpSecurity::StartTls,
        }
    }

    fn dkim_test_outbound() -> OutboundEmailMessage<'static> {
        OutboundEmailMessage {
            to: "alice@example.com",
            subject: "[#42] hi",
            body_text: "hi",
            body_html: None,
            message_id: "ticket-42.comment-1.aaaaaaaa@example.org",
            in_reply_to: None,
            references: &[],
            auto_submitted: None,
            mail_class: "transactional",
            reply_to: None,
            envelope_from: None,
            list_unsubscribe: None,
        }
    }

    #[test]
    fn dkim_signature_is_added_when_signer_present() {
        let mut msg = build_outbound_message(&dkim_test_config(), &dkim_test_outbound()).unwrap();
        let signer = DkimSigner {
            selector: "test".into(),
            domain: "example.org".into(),
            private_key: TEST_DKIM_PKCS1_KEY.into(),
            algorithm: DkimAlgorithm::Rsa,
        };
        dkim_sign_message(&mut msg, &signer).unwrap();
        let dump = String::from_utf8(msg.formatted()).expect("utf-8");
        assert!(
            dump.contains("DKIM-Signature:"),
            "missing DKIM-Signature:\n{dump}"
        );
        assert!(
            dump.contains("d=example.org"),
            "wrong signing domain:\n{dump}"
        );
        assert!(dump.contains("s=test"), "wrong selector:\n{dump}");
        assert!(
            dump.contains("a=rsa-sha256"),
            "wrong algorithm tag:\n{dump}"
        );
        // Relaxed/relaxed canonicalization so the signature survives a relay
        // refolding signed headers in transit (not lettre's default `simple`).
        assert!(
            dump.contains("c=relaxed/relaxed"),
            "expected relaxed/relaxed canonicalization:\n{dump}"
        );
        // Expanded signed-header set: identity + framing + threading, beyond
        // the default From/Subject/To/Date. The h= tag is lowercased by relaxed.
        let h = dump
            .split("h=")
            .nth(1)
            .and_then(|s| s.split(';').next())
            .unwrap_or("")
            .to_ascii_lowercase();
        for required in [
            "from",
            "subject",
            "to",
            "date",
            "message-id",
            "mime-version",
            "content-type",
        ] {
            assert!(h.contains(required), "h= missing {required}:\n{dump}");
        }
    }

    #[test]
    fn dkim_invalid_key_is_a_clean_error() {
        let mut msg = build_outbound_message(&dkim_test_config(), &dkim_test_outbound()).unwrap();
        let signer = DkimSigner {
            selector: "s".into(),
            domain: "example.org".into(),
            private_key: "-----BEGIN RSA PRIVATE KEY-----\nnope\n-----END RSA PRIVATE KEY-----"
                .into(),
            algorithm: DkimAlgorithm::Rsa,
        };
        let err = dkim_sign_message(&mut msg, &signer).unwrap_err();
        assert!(
            err.contains("invalid DKIM signing key"),
            "unexpected: {err}"
        );
    }

    #[tokio::test]
    async fn untrusted_relay_rejects_internal_host_before_connecting() {
        // An untrusted (smtp_relay) transport pointed at a loopback host must be
        // refused by the SSRF resolve+validate step, not dialed. 127.0.0.1 is an
        // IP literal so resolve_and_validate hits no network DNS — it resolves
        // locally and the routability check rejects it. Deterministic; the egress
        // allowlist never contains 127.0.0.1.
        let mut config = dkim_test_config();
        config.smtp_host = "127.0.0.1".into();
        let transport = SmtpEmailTransport::new_untrusted(config, UNTRUSTED_RELAY_TIMEOUT);
        let outbound = dkim_test_outbound();
        let err = match transport.send(&outbound).await {
            Err(e) => e,
            Ok(_) => panic!("expected the internal relay host to be rejected"),
        };
        assert!(
            matches!(err, SmtpError::Egress(_)),
            "expected an SSRF/egress rejection, got: {err}"
        );
    }

    /// A variable source for `EmailConfig::from_lookup`.
    fn smtp_vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    /// A one-message SMTP server on an ephemeral port. It advertises AUTH, as
    /// a relay that also takes submissions would, and returns every command
    /// the client sent.
    fn smtp_sink() -> (u16, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut out = stream;
            out.write_all(b"220 sink ESMTP\r\n").unwrap();
            let mut commands = Vec::new();
            let mut in_data = false;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if in_data {
                    if line == ".\r\n" {
                        in_data = false;
                        out.write_all(b"250 queued\r\n").unwrap();
                    }
                    continue;
                }
                let command = line.trim_end().to_string();
                let verb = command.to_ascii_uppercase();
                commands.push(command);
                let reply: &[u8] = if verb.starts_with("EHLO") {
                    b"250-sink\r\n250-AUTH PLAIN LOGIN\r\n250 8BITMIME\r\n"
                } else if verb.starts_with("DATA") {
                    in_data = true;
                    b"354 go ahead\r\n"
                } else if verb.starts_with("AUTH") {
                    b"235 accepted\r\n"
                } else if verb.starts_with("QUIT") {
                    let _ = out.write_all(b"221 bye\r\n");
                    break;
                } else {
                    b"250 ok\r\n"
                };
                out.write_all(reply).unwrap();
            }
            commands
        });
        (port, handle)
    }

    #[tokio::test]
    async fn a_relay_that_authorises_by_address_needs_no_credentials() {
        // The office relay on port 25: a host and a From address are enough.
        let config = EmailConfig::from_lookup(smtp_vars(&[
            ("SMTP_ENABLED", "true"),
            ("SMTP_HOST", "relay.example.com"),
            ("SMTP_PORT", "25"),
            ("SMTP_FROM_EMAIL", "help@example.com"),
        ]))
        .expect("a relay without credentials is a valid configuration");
        assert!(config.is_configured());
        assert!(EmailService::new(config.clone()).is_configured());
        assert!(!config.signs_in(), "nothing to sign in with");

        // And it sends: no AUTH, though the server offers it.
        let (port, sink) = smtp_sink();
        let port = port.to_string();
        let config = EmailConfig::from_lookup(smtp_vars(&[
            ("SMTP_ENABLED", "true"),
            ("SMTP_HOST", "127.0.0.1"),
            ("SMTP_PORT", &port),
            ("SMTP_SECURITY", "plaintext"),
            ("SMTP_FROM_EMAIL", "help@example.com"),
        ]))
        .unwrap();
        SmtpEmailTransport::new(config)
            .send(&dkim_test_outbound())
            .await
            .expect("sent");
        let commands = sink.join().unwrap();
        assert!(
            commands
                .iter()
                .any(|c| c.starts_with("MAIL FROM:<help@example.com>")),
            "{commands:?}"
        );
        assert!(
            !commands
                .iter()
                .any(|c| c.to_ascii_uppercase().starts_with("AUTH")),
            "{commands:?}"
        );
    }

    #[test]
    fn one_smtp_credential_alone_is_a_config_error() {
        let base = [
            ("SMTP_ENABLED", "true"),
            ("SMTP_HOST", "relay.example.com"),
            ("SMTP_FROM_EMAIL", "help@example.com"),
        ];
        let with = |extra: (&str, &str)| {
            let mut pairs = base.to_vec();
            pairs.push(extra);
            EmailConfig::from_lookup(smtp_vars(&pairs)).unwrap_err()
        };
        let no_password = with(("SMTP_USERNAME", "help@example.com"));
        assert!(no_password.contains("SMTP_PASSWORD"), "{no_password}");
        assert!(no_password.contains("neither"), "{no_password}");
        let no_username = with(("SMTP_PASSWORD", "secret"));
        assert!(no_username.contains("SMTP_USERNAME"), "{no_username}");
        assert!(no_username.contains("neither"), "{no_username}");

        // Both: signs in.
        let mut pairs = base.to_vec();
        pairs.push(("SMTP_USERNAME", "help@example.com"));
        pairs.push(("SMTP_PASSWORD", "secret"));
        let config = EmailConfig::from_lookup(smtp_vars(&pairs)).unwrap();
        assert!(config.signs_in());
        // Not over a plaintext connection, which can't carry them.
        pairs.push(("SMTP_SECURITY", "plaintext"));
        let config = EmailConfig::from_lookup(smtp_vars(&pairs)).unwrap();
        assert!(!config.signs_in());
    }

    #[test]
    fn the_from_address_falls_back_only_to_a_username_that_is_one() {
        let from = |username: &str| {
            EmailConfig::from_lookup(smtp_vars(&[
                ("SMTP_ENABLED", "true"),
                ("SMTP_HOST", "smtp.example.com"),
                ("SMTP_USERNAME", username),
                ("SMTP_PASSWORD", "secret"),
            ]))
            .map(|c| c.from_email)
        };
        assert_eq!(from("help@example.com").unwrap(), "help@example.com");
        // An API-key login is no address to send from.
        let err = from("apikey").unwrap_err();
        assert!(err.contains("SMTP_FROM_EMAIL"), "{err}");
    }

    #[test]
    fn either_relay_can_send_without_credentials() {
        let mut config = dkim_test_config();
        config.smtp_host = "relay.example.com".into();
        config.smtp_username = String::new();
        config.smtp_password = String::new();
        // An IP-allowlisted office relay: a host is enough.
        assert!(
            SmtpEmailTransport::new_untrusted(config.clone(), UNTRUSTED_RELAY_TIMEOUT).can_send()
        );
        // The operator's env relay too, given a From address.
        assert!(SmtpEmailTransport::new(config.clone()).can_send());
        let mut fromless = config.clone();
        fromless.from_email = String::new();
        assert!(!SmtpEmailTransport::new(fromless).can_send());
        // A workspace relay still needs a host and to be enabled.
        let mut hostless = config.clone();
        hostless.smtp_host = " ".into();
        assert!(!SmtpEmailTransport::new_untrusted(hostless, UNTRUSTED_RELAY_TIMEOUT).can_send());
        config.enabled = false;
        assert!(!SmtpEmailTransport::new_untrusted(config, UNTRUSTED_RELAY_TIMEOUT).can_send());
    }

    #[tokio::test]
    async fn trusted_relay_does_not_ssrf_validate_the_host() {
        // The env/verified-domain relay is operator config: a loopback host is a
        // legitimate self-host relay and must NOT be SSRF-rejected. It will fail
        // to connect (nothing is listening), but the error must be a connection
        // failure, not the SSRF rejection.
        let mut config = dkim_test_config();
        config.smtp_host = "127.0.0.1".into();
        config.smtp_port = 59; // almost certainly closed
        let transport = SmtpEmailTransport::new(config);
        let outbound = dkim_test_outbound();
        let err = match transport.send(&outbound).await {
            Err(e) => e,
            Ok(_) => panic!("expected a connection failure to the closed port"),
        };
        assert!(
            !matches!(err, SmtpError::Egress(_)),
            "trusted relay must not be SSRF-rejected, got: {err}"
        );
    }

    #[test]
    fn dkim_signer_debug_redacts_private_key() {
        let signer = DkimSigner {
            selector: "test".into(),
            domain: "example.org".into(),
            private_key: "SUPER-SECRET-KEY".into(),
            algorithm: DkimAlgorithm::Rsa,
        };
        let dbg = format!("{signer:?}");
        assert!(
            !dbg.contains("SUPER-SECRET-KEY"),
            "private key leaked in Debug: {dbg}"
        );
        assert!(dbg.contains("redacted"));
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
                mail_class: "transactional",
                reply_to: None,
                envelope_from: None,
                list_unsubscribe: None,
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
    fn reply_to_header_emitted_only_when_set() {
        let base = OutboundEmailMessage {
            to: "alice@example.com",
            subject: "[#42] Re: thread",
            body_text: "reply",
            body_html: None,
            message_id: "ticket-42.comment-3.cafef00d@yourco.com",
            in_reply_to: None,
            references: &[],
            auto_submitted: None,
            mail_class: "transactional",
            reply_to: Some("support@acme.com"),
            envelope_from: None,
            list_unsubscribe: None,
        };
        let with = rendered(&svc().build_ticket_reply_message(&base).unwrap());
        assert!(with.contains("Reply-To: support@acme.com"), "dump:\n{with}");

        let without = rendered(
            &svc()
                .build_ticket_reply_message(&OutboundEmailMessage {
                    reply_to: None,
                    envelope_from: None,
                    list_unsubscribe: None,
                    ..base
                })
                .unwrap(),
        );
        assert!(!without.contains("Reply-To:"), "dump:\n{without}");
    }

    #[test]
    fn list_unsubscribe_headers_emitted_only_when_set() {
        let base = OutboundEmailMessage {
            to: "alice@example.com",
            subject: "Ticket #42 updated",
            body_text: "an update",
            body_html: None,
            message_id: "notify.cafef00d@yourco.com",
            in_reply_to: None,
            references: &[],
            auto_submitted: None,
            mail_class: "notification",
            reply_to: None,
            envelope_from: None,
            list_unsubscribe: Some("https://acme.nosdesk.dev/api/public/unsubscribe?token=t.sig"),
        };
        let with = rendered(&svc().build_ticket_reply_message(&base).unwrap());
        assert!(
            with.contains(
                "List-Unsubscribe: <https://acme.nosdesk.dev/api/public/unsubscribe?token=t.sig>"
            ),
            "dump:\n{with}"
        );
        assert!(
            with.contains("List-Unsubscribe-Post: List-Unsubscribe=One-Click"),
            "dump:\n{with}"
        );

        let without = rendered(
            &svc()
                .build_ticket_reply_message(&OutboundEmailMessage {
                    list_unsubscribe: None,
                    ..base
                })
                .unwrap(),
        );
        assert!(!without.contains("List-Unsubscribe"), "dump:\n{without}");
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
                mail_class: "transactional",
                reply_to: None,
                envelope_from: None,
                list_unsubscribe: None,
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
            mail_class: "transactional",
            reply_to: None,
            envelope_from: None,
            list_unsubscribe: None,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(disabled.send_ticket_reply(outbound))
            .unwrap_err();
        assert!(err.contains("not configured"), "unexpected error: {err}");
    }

    /// The plain-text part of every letter, one per builder.
    fn every_letter_text(
        svc: &EmailService,
        branding: &EmailBranding,
    ) -> Vec<(&'static str, String)> {
        use std::str::FromStr;
        let locale = unic_langid::LanguageIdentifier::from_str("en-US").unwrap();
        vec![
            (
                "password reset",
                svc.compose_password_reset("Alex", "TOKEN", branding, &locale)
                    .2,
            ),
            (
                "invitation",
                svc.compose_invitation("Alex", "TOKEN", branding, "Kyle", &locale)
                    .2,
            ),
            (
                "participant added",
                svc.compose_participant_added(
                    "Kyle",
                    42,
                    "Printer fire",
                    "https://desk.example.com/tickets/42",
                    branding,
                    &locale,
                )
                .2,
            ),
            (
                "portal sign-in",
                svc.compose_portal_magic_link("Alex", "TOKEN", Some("123456"), branding, &locale)
                    .2,
            ),
            (
                "email verification",
                svc.compose_email_verification(
                    "Alex",
                    "alex@example.com",
                    "TOKEN",
                    branding,
                    &locale,
                )
                .2,
            ),
            (
                "guest ticket confirmation",
                svc.compose_guest_ticket_confirmation("TOKEN", branding, None)
                    .2,
            ),
            (
                "notification",
                svc.compose_notification(
                    "New comment on: Printer fire",
                    "Can someone take a look?",
                    "Kyle",
                    "https://desk.example.com/tickets/42",
                    None,
                    None,
                    branding,
                    &locale,
                )
                .1,
            ),
            (
                "test email",
                super::test_letter(
                    &super::EmailTemplate::new(branding),
                    &locale,
                    Some(("smtp.example.com:587", "Acme <support@acme.example.com>")),
                )
                .2,
            ),
        ]
    }

    #[test]
    fn every_letter_ends_its_plain_text_with_the_security_note() {
        let svc = svc();
        let mut branding = EmailBranding::default();
        let note = "Acme will only ever email you from acme.example.com.";
        branding.security_note = Some(note.to_string());
        for (letter, text) in every_letter_text(&svc, &branding) {
            assert!(
                text.trim_end().ends_with(note),
                "{letter}: plain text should end with the note:\n{text}"
            );
        }

        branding.security_note = None;
        for (letter, text) in every_letter_text(&svc, &branding) {
            assert!(
                !text.contains("will only ever email you"),
                "{letter}: no note when it's off:\n{text}"
            );
        }
    }

    /// `every_letter_text` builds every `compose_` letter, so the security
    /// note test above covers each one a new builder adds.
    #[test]
    fn every_letter_text_builds_every_compose_letter() {
        let src = include_str!("email.rs");
        let start = src
            .find("fn every_letter_text(")
            .expect("every_letter_text");
        let end = start + src[start..].find("\n    }\n").expect("its end");
        let listed = &src[start..end];
        let builder = regex::Regex::new(r"(?m)^\s*pub fn (compose_\w+)\(").unwrap();
        let missing: Vec<&str> = builder
            .captures_iter(src)
            .map(|c| c.get(1).expect("name").as_str())
            .filter(|name| !listed.contains(&format!("{name}(")))
            .collect();
        assert!(
            missing.is_empty(),
            "add these letters to every_letter_text: {missing:?}"
        );
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

        let (_subj, html, text) = svc.compose_guest_ticket_confirmation(
            "EXAMPLE-GUEST-TOKEN",
            &branding,
            Some("https://acme.example.com/api/public/email/stop?t=EXAMPLE"),
        );
        assert!(
            html.contains("email/stop?t=EXAMPLE"),
            "carries the stop link"
        );
        assert!(text.contains("Confirm"), "has a plain-text part");
        write("guest-ticket-confirmation", &html);

        let (html, _text) = svc.compose_notification(
            "New comment on: Printer fire",
            "It's still burning. Can someone take a look?",
            "Kyle",
            "https://desk.example.com/tickets/42",
            None,
            None,
            &branding,
            &locale,
        );
        write("notification", &html);

        let (html, text) = svc.compose_notification(
            "Your request was updated",
            "Request #42 is now Done.",
            "Kyle",
            "https://acme.example.com/api/portal/auth/ticket?t=EXAMPLE",
            Some("View request"),
            Some(&FeedbackLinks {
                fixed: "https://acme.example.com/api/portal/auth/ticket?t=EXAMPLE&answer=fixed"
                    .into(),
                not_fixed:
                    "https://acme.example.com/api/portal/auth/ticket?t=EXAMPLE&answer=not_fixed"
                        .into(),
            }),
            &branding,
            &locale,
        );
        assert!(html.contains("answer=fixed") && html.contains("answer=not_fixed"));
        assert!(text.contains("answer=not_fixed"));
        write("notification-resolved", &html);

        let (html, text) = svc.compose_notification(
            "Request received",
            "We got your request.\n\nWe're aware of an issue that may be related: Email is down\nFixing now.",
            "",
            "https://acme.example.com/api/portal/auth/ticket?t=EXAMPLE",
            Some("View request"),
            None,
            &branding,
            &locale,
        );
        assert_eq!(
            html.matches(r#"class="nd-body""#).count(),
            2,
            "one paragraph each"
        );
        assert!(text.contains("Email is down"));
        write("notification-ack-known-issue", &html);

        let (_subj, html, text) = svc.compose_portal_magic_link(
            "Alex",
            "EXAMPLE-SIGNIN-TOKEN",
            Some("123456"),
            &branding,
            &locale,
        );
        write("portal-magic-link", &html);
        // The CTA must carry the portal callback link on the configured origin.
        assert!(
            html.contains("/api/portal/auth/callback?token=EXAMPLE-SIGNIN-TOKEN"),
            "magic-link html must link to the portal callback"
        );
        assert!(
            text.contains("/api/portal/auth/callback?token=EXAMPLE-SIGNIN-TOKEN"),
            "magic-link plaintext must carry the callback link"
        );

        // Sanity: every preview file exists and is non-trivial.
        for name in [
            "password-reset",
            "invitation",
            "guest-ticket-confirmation",
            "notification",
            "portal-magic-link",
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
