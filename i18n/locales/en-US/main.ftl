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
invitation-subject = You've Been Invited to { $app } - Set Up Your Account

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
settings-timezone-browser-detected = Browser-detected ({ $tz })
settings-save = Save
settings-saving = Saving...
settings-localization-saved = Language and timezone preferences saved
settings-localization-save-failed = Failed to save preferences
