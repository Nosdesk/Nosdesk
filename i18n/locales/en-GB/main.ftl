## Shared Fluent message catalogue for en-GB.
##
## en-GB diverges from en-US only where spelling or wording
## materially changes (organise vs organize, customise vs
## customize, postcode vs zip code). Unchanged keys fall back to
## en-US via the negotiator.

greeting = Hello, { $name }.
unread-count = { $count ->
    [0] No new messages.
    [one] One new message.
   *[other] { $count } new messages.
}

password-reset-subject = Reset Your { $app } Password
invitation-subject = You've Been Invited to { $app } - Set Up Your Account

notif-ticket-assigned = [{ $app }] You've been assigned: { $title }
notif-ticket-status-changed = [{ $app }] Status changed: { $title }
notif-comment-added = [{ $app }] New comment on: { $title }
notif-mentioned = [{ $app }] { $actor } mentioned you
notif-ticket-created-requester = [{ $app }] Ticket created: { $title }
notif-doc-page-updated = [{ $app }] Page updated: { $title }

# en-GB: same wording as en-US for now; this section exists so the
# locale ships its own catalogue end-to-end. Spelling differences
# (organise / customise) will land here when those keys are added.
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
