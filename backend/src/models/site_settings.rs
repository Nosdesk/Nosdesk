use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Site Settings - Branding and Customization
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::site_settings)]
pub struct SiteSettings {
    pub id: i32,
    pub app_name: String,
    pub logo_url: Option<String>,
    pub logo_light_url: Option<String>,
    pub favicon_url: Option<String>,
    pub primary_color: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub updated_by: Option<Uuid>,
    pub guest_tickets_enabled: bool,
    pub guest_public_docs_enabled: bool,
    pub guest_kb_search_enabled: bool,
    pub guest_ticket_lookup_enabled: bool,
    pub guest_help_page_enabled: bool,
    pub guest_ticket_default_priority: Option<String>,
    pub guest_ticket_rate_limit_per_hour: i32,
    pub guest_ticket_email_verification: bool,
    pub guest_ticket_attachments_enabled: bool,
    pub guest_ticket_intro_message: Option<String>,
    /// Whether to send a one-off "thanks, we got your message" reply
    /// when a channel message opens a fresh ticket. Defaults true.
    pub channel_auto_ack_enabled: bool,
    /// Admin-overridden template for the auto-ack body. `None` uses
    /// the built-in default (see
    /// [`crate::services::channels::auto_ack::DEFAULT_TEMPLATE`]).
    pub channel_auto_ack_template: Option<String>,
    /// Workspace-level feature flag defaults. JSONB shape
    /// `{ "<flag_name>": <boolean | string | object>, ... }`. Empty
    /// object = all flags at code-default. Per-user overrides on the
    /// `users` table merge on top at request time.
    pub feature_flags: serde_json::Value,
    /// System-wide default BCP-47 locale used when the user has no
    /// preference and (for guests) when the inbound mail's
    /// `Content-Language` was missing or unsupported. Defaults to
    /// `en-US`; operator can change via admin settings.
    pub default_locale: String,
    /// System-wide default IANA timezone used when the user has no
    /// preference. Defaults to `UTC`; operator typically sets this
    /// to the team's working zone (e.g. `Australia/Sydney`).
    pub default_timezone: String,
    pub workspace_id: i32,
    /// Workspace-wide default email signature. The outbound channel
    /// reply pipeline appends this when an agent has not set a
    /// personal signature in `user_preferences.signature`. `None` =
    /// no org default; reply goes out unsigned, matching the pre-
    /// migration behaviour.
    pub signature_default: Option<String>,
    /// Whether to render the anti-phishing security note in the
    /// transactional email footer. Defaults false: the note is
    /// brand-specific, so it stays opt-in until an admin enables it.
    pub email_security_note_enabled: bool,
    /// Admin-overridden security-note body. `None` uses the built-in
    /// localized default (FTL key `email-security-note-default`).
    /// Supports `{{app_name}}` and `{{domain}}` placeholders.
    pub email_security_note_template: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::site_settings)]
pub struct UpdateSiteSettings {
    pub app_name: Option<String>,
    pub logo_url: Option<Option<String>>,
    pub logo_light_url: Option<Option<String>>,
    pub favicon_url: Option<Option<String>>,
    pub primary_color: Option<Option<String>>,
    pub updated_by: Option<Uuid>,
    pub guest_tickets_enabled: Option<bool>,
    pub guest_public_docs_enabled: Option<bool>,
    pub guest_kb_search_enabled: Option<bool>,
    pub guest_ticket_lookup_enabled: Option<bool>,
    pub guest_help_page_enabled: Option<bool>,
    pub guest_ticket_default_priority: Option<Option<String>>,
    pub guest_ticket_rate_limit_per_hour: Option<i32>,
    pub guest_ticket_email_verification: Option<bool>,
    pub guest_ticket_attachments_enabled: Option<bool>,
    pub guest_ticket_intro_message: Option<Option<String>>,
    pub channel_auto_ack_enabled: Option<bool>,
    pub channel_auto_ack_template: Option<Option<String>>,
    pub default_locale: Option<String>,
    pub default_timezone: Option<String>,
    /// `Option<Option<String>>`: outer `None` = leave as-is,
    /// `Some(None)` = clear back to NULL (no org default),
    /// `Some(Some(_))` = set the org-wide template.
    pub signature_default: Option<Option<String>>,
    pub email_security_note_enabled: Option<bool>,
    /// Same `Option<Option<String>>` clear semantics as the auto-ack
    /// template: `Some(None)` reverts to the built-in default.
    pub email_security_note_template: Option<Option<String>>,
}

// API response for site settings (without internal fields)
#[derive(Debug, Serialize, Deserialize)]
pub struct SiteSettingsResponse {
    pub app_name: String,
    pub logo_url: Option<String>,
    pub logo_light_url: Option<String>,
    pub favicon_url: Option<String>,
    pub primary_color: Option<String>,
    pub updated_at: NaiveDateTime,
    pub guest_tickets_enabled: bool,
    pub guest_public_docs_enabled: bool,
    pub guest_kb_search_enabled: bool,
    pub guest_ticket_lookup_enabled: bool,
    pub guest_help_page_enabled: bool,
    pub guest_ticket_default_priority: Option<String>,
    pub guest_ticket_rate_limit_per_hour: i32,
    pub guest_ticket_email_verification: bool,
    pub guest_ticket_attachments_enabled: bool,
    pub guest_ticket_intro_message: Option<String>,
    /// Workspace-wide default email signature. Admin-visible only;
    /// excluded from `PublicSiteSettings` since it isn't relevant
    /// to anonymous guest views.
    pub signature_default: Option<String>,
    /// Whether to send the "we got your message" auto-acknowledgement
    /// when a channel message opens a new ticket. See
    /// `services::channels::auto_ack`.
    pub channel_auto_ack_enabled: bool,
    /// Admin-overridden template for the auto-ack body. `None` =
    /// use the built-in FTL default for the resolved locale.
    pub channel_auto_ack_template: Option<String>,
    /// Whether the anti-phishing security note renders in the email
    /// footer. See `utils::email_branding::resolve_security_note`.
    pub email_security_note_enabled: bool,
    /// Admin-overridden security-note body. `None` = use the built-in
    /// localized default.
    pub email_security_note_template: Option<String>,
}

impl From<SiteSettings> for SiteSettingsResponse {
    fn from(settings: SiteSettings) -> Self {
        SiteSettingsResponse {
            app_name: settings.app_name,
            logo_url: settings.logo_url,
            logo_light_url: settings.logo_light_url,
            favicon_url: settings.favicon_url,
            primary_color: settings.primary_color,
            updated_at: settings.updated_at,
            guest_tickets_enabled: settings.guest_tickets_enabled,
            guest_public_docs_enabled: settings.guest_public_docs_enabled,
            guest_kb_search_enabled: settings.guest_kb_search_enabled,
            guest_ticket_lookup_enabled: settings.guest_ticket_lookup_enabled,
            guest_help_page_enabled: settings.guest_help_page_enabled,
            guest_ticket_default_priority: settings.guest_ticket_default_priority,
            guest_ticket_rate_limit_per_hour: settings.guest_ticket_rate_limit_per_hour,
            guest_ticket_email_verification: settings.guest_ticket_email_verification,
            guest_ticket_attachments_enabled: settings.guest_ticket_attachments_enabled,
            guest_ticket_intro_message: settings.guest_ticket_intro_message,
            signature_default: settings.signature_default,
            channel_auto_ack_enabled: settings.channel_auto_ack_enabled,
            channel_auto_ack_template: settings.channel_auto_ack_template,
            email_security_note_enabled: settings.email_security_note_enabled,
            email_security_note_template: settings.email_security_note_template,
        }
    }
}

// Public subset — safe to expose on /api/public/settings (no auth required)
#[derive(Debug, Serialize, Deserialize)]
pub struct PublicSiteSettings {
    pub app_name: String,
    pub logo_url: Option<String>,
    pub logo_light_url: Option<String>,
    pub favicon_url: Option<String>,
    pub primary_color: Option<String>,
    pub guest_tickets_enabled: bool,
    pub guest_public_docs_enabled: bool,
    pub guest_kb_search_enabled: bool,
    pub guest_ticket_lookup_enabled: bool,
    pub guest_help_page_enabled: bool,
    pub guest_ticket_attachments_enabled: bool,
    pub guest_ticket_intro_message: Option<String>,
}

impl From<&SiteSettings> for PublicSiteSettings {
    fn from(s: &SiteSettings) -> Self {
        PublicSiteSettings {
            app_name: s.app_name.clone(),
            logo_url: s.logo_url.clone(),
            logo_light_url: s.logo_light_url.clone(),
            favicon_url: s.favicon_url.clone(),
            primary_color: s.primary_color.clone(),
            guest_tickets_enabled: s.guest_tickets_enabled,
            guest_public_docs_enabled: s.guest_public_docs_enabled,
            guest_kb_search_enabled: s.guest_kb_search_enabled,
            guest_ticket_lookup_enabled: s.guest_ticket_lookup_enabled,
            guest_help_page_enabled: s.guest_help_page_enabled,
            guest_ticket_attachments_enabled: s.guest_ticket_attachments_enabled,
            guest_ticket_intro_message: s.guest_ticket_intro_message.clone(),
        }
    }
}
