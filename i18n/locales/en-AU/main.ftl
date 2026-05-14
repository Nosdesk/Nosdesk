## Shared Fluent message catalogue for en-AU.
##
## en-AU follows en-GB spelling (organise / customise) but may
## diverge on idiom and date-shorthand. Unchanged keys fall back
## via the negotiator: en-AU -> en-GB -> en-US.

greeting = G'day, { $name }.
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

# en-AU diverges from en-US on a couple of words ("Timezone" stays
# the same; the help copy nudges to AU phrasing). The Save button
# label is slightly different to make the locale flip visibly
# obvious during dev verification.
settings-localization-title = Language & Time Zone
settings-localization-help = Sets the language for messages and how dates render. Site default applies if you don't choose one.
settings-language-label = Language
settings-timezone-label = Time Zone
settings-locale-site-default = Site default
settings-locale-en-US = English (United States)
settings-locale-en-GB = English (United Kingdom)
settings-locale-en-AU = English (Australia)
settings-timezone-browser-detected = Browser-detected ({ $tz })
settings-save = Save
settings-saving = Saving...
settings-localization-saved = Language and time zone preferences saved
settings-localization-save-failed = Couldn't save preferences
