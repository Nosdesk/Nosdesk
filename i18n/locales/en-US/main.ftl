## Shared Fluent message catalogue for en-US.
##
## Loaded by both the Rust backend (via include_str! in
## backend/src/utils/i18n.rs) and the Vue frontend (via Vite glob
## import in frontend/src/i18n/index.ts). The .ftl format is
## Mozilla's Fluent. Keep keys kebab-case, group by feature
## prefix (e.g. password-reset-*, ticket-*) so call sites stay
## predictable.

# Generic
greeting = Hello, { $name }.
unread-count = { $count ->
    [0] No new messages.
    [one] One new message.
   *[other] { $count } new messages.
}

# Transactional email subjects. $app interpolates the configured
# workspace name from EmailBranding so the line reads "Reset Your
# Acme Helpdesk Password" rather than the literal product name.
password-reset-subject = Reset Your { $app } Password
# Password-reset email body. The HTML strings carry inline
# <strong> tags for typographic emphasis; translators preserve
# them as opaque markers around the words they wrap. The plain-
# text variant is one Fluent multi-line value so translators see
# the full prose at once. $name and $app are already HTML-escaped
# by the caller in HTML contexts and passed raw in plaintext.
password-reset-title = Password Reset Request
password-reset-greeting = Hello <strong>{ $name }</strong>,
password-reset-intro = We received a request to reset your password for your <strong>{ $app }</strong> account. If you didn't make this request, you can safely ignore this email.
password-reset-action-prompt = To reset your password, click the button below:
password-reset-cta-label = Reset Password
password-reset-notice-expiry = This link will expire in <strong>1 hour</strong>
password-reset-notice-single-use = This link can only be used <strong>once</strong>
password-reset-notice-never-share = Never share this link with anyone
password-reset-notice-account-security = If you didn't request this reset, please secure your account immediately
password-reset-footer = If you have any questions, please contact your system administrator.
password-reset-body-text =
    Hello { $name },

    We received a request to reset your password for your { $app } account. If you didn't make this request, you can safely ignore this email.

    To reset your password, open this link in your browser:

    { $link }

    Security notes:
      - This link will expire in 1 hour.
      - This link can only be used once.
      - Never share this link with anyone.
      - If you didn't request this reset, secure your account.

    If you have any questions, please contact your system administrator.

    -- { $app }
invitation-subject = You've Been Invited to { $app } - Set Up Your Account
# Invitation email body. HTML keys carry inline <strong>
# emphasis around variables; variables themselves are HTML-
# escaped at the Rust boundary. Plaintext body is one Fluent
# multi-line value carrying the same prose.
invitation-title = Welcome to { $app }!
invitation-greeting = Hello <strong>{ $name }</strong>,
invitation-intro = You've been invited to join <strong>{ $app }</strong> by <strong>{ $by }</strong>.
invitation-action-prompt = To complete your account setup and create your password, click the button below:
invitation-cta-label = Set Up Your Account
invitation-notice-expiry = This invitation link will expire in <strong>7 days</strong>
invitation-notice-create-password = You'll need to create a password during setup
invitation-notice-strong-password = Choose a strong password with at least 8 characters
invitation-notice-unexpected = If you didn't expect this invitation, you can safely ignore this email
invitation-footer = If you have any questions, please contact your system administrator.
invitation-body-text =
    Hello { $name },

    You've been invited to join { $app } by { $by }.

    To complete your account setup and create your password, open this link in your browser:

    { $link }

    A few things to know:
      - This invitation will expire in 7 days.
      - You'll create a password during setup.
      - Choose a strong password with at least 8 characters.
      - If you didn't expect this invitation, you can safely ignore this email.

    -- { $app }

# Notification email subjects. Each stamps the workspace name in
# brackets so inbox grouping by sender + subject prefix still
# works. $title is the entity (ticket title / doc page title);
# $actor only fires for the Mentioned variant.
notif-ticket-assigned = [{ $app }] You've been assigned: { $title }
notif-ticket-status-changed = [{ $app }] Status changed: { $title }
notif-comment-added = [{ $app }] New comment on: { $title }
notif-mentioned = [{ $app }] { $actor } mentioned you
notif-ticket-created-requester = [{ $app }] Ticket created: { $title }
notif-doc-page-updated = [{ $app }] Page updated: { $title }
# Notification email body. The user-authored payload (`body`)
# stays verbatim, escaped at the Rust boundary. Only the
# connector copy below gets translated.
notif-body-fallback = You have a new notification.
notif-from-row = <strong>From:</strong> { $actor }
notif-cta-view-in = View in { $app }
notif-footer-preferences = You're receiving this because of your notification preferences.
notif-body-text =
    { $title }

    { $body }

    From: { $actor }

    View in { $app }: { $cta }

    -- You're receiving this because of your notification preferences in { $app }.

# Localization settings panel. Every string in
# components/settings/LocalizationSettings.vue resolves through
# this section, so flipping the active locale immediately re-
# renders the picker that flipped it.
settings-localization-title = Language & Timezone
settings-localization-help = Affects message language and how dates render. Site default applies when you don't pick one explicitly.
settings-language-label = Language
settings-timezone-label = Timezone
settings-locale-site-default = Site default
settings-locale-en-US = English (United States)
settings-locale-en-GB = English (United Kingdom)
settings-locale-en-AU = English (Australia)
settings-locale-fr-FR = French (France)
settings-locale-nl-NL = Dutch (Netherlands)
settings-timezone-browser-detected = Browser-detected ({ $tz })
settings-timezone-use-device = Use device timezone
settings-timezone-search-placeholder = Search city or offset (e.g. Sydney, UTC+10)
settings-timezone-no-matches = No timezones match that search
settings-save = Save
settings-saving = Saving...
settings-localization-saved = Language and timezone preferences saved
settings-localization-save-failed = Failed to save preferences

# Default body for the channel auto-acknowledgement reply when no
# admin-customised template is set. Picked by the inbound's
# Content-Language so a French-written ticket gets a French ack.
# Admin customisation in `site_settings.channel_auto_ack_template`
# bypasses this entirely (custom copy is the source of truth).
auto-ack-default-template = Your request (#{ $ticket_id }) has been received and is being reviewed by our support team. To add additional comments, reply to this email.

# Inbox-time connecting copy. The bare time string ("3:42 PM"),
# weekday ("Mon"), and relative phrases ("5 minutes ago") all
# come from Intl.DateTimeFormat / Intl.RelativeTimeFormat in the
# active locale; these keys are just the glue that strings them
# together for "today" / "yesterday" / "this week" buckets.
inbox-time-just-now = Just now
inbox-time-yesterday = Yesterday at { $time }
inbox-time-weekday = { $day } at { $time }
