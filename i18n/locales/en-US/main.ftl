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

# Login + MFA challenge view. First contact for every user;
# the strings here are deliberately conservative — short
# CTAs, clear field labels, no idioms a translator has to
# wrestle with.
login-subtitle = Sign in to your account
login-email-label = Email
login-email-placeholder = Enter your email
login-password-label = Password
login-password-placeholder = Enter your password
login-password-show = Show password
login-password-hide = Hide password
login-forgot-password = Forgot password?
login-submit = Sign in
login-submitting = Signing in...
login-passkey-cta = Sign in with passkey
login-passkey-authenticating = Authenticating...
login-microsoft-cta = Sign in with Microsoft Entra
login-microsoft-connecting = Connecting...
login-microsoft-logout-title = Sign out of Microsoft account
login-oidc-cta = Sign in with { $provider }
login-oidc-logout-title = Sign out of { $provider } account
login-oidc-connecting = Connecting...
login-divider-or = or
login-mfa-title = Two-Factor Authentication
login-mfa-subtitle = Please enter your authentication code
login-mfa-code-label = Authentication Code
login-mfa-code-help = Enter the 6-digit code from your authenticator app or an 8-character backup code
login-mfa-back = Back
login-mfa-verify = Verify & Sign In
login-mfa-verifying = Verifying...
login-passkey-mfa-verified = Password verified for { $email }
login-passkey-mfa-verify-cta = Verify with passkey
login-passkey-mfa-use-recovery = Use a recovery code
login-passkey-mfa-back-to-login = Back to login
login-recovery-code-label = Recovery Code
login-recovery-code-placeholder = Enter recovery code
login-recovery-code-help = Enter one of the 8-character recovery codes you saved during setup

# Forgot-password modal — opens from the LoginView "Forgot
# password?" link. Submits an email, then shows a success state
# with rate-limit + spam-folder reminders.
forgot-password-title = Reset Your Password
forgot-password-close-modal = Close modal
forgot-password-intro = Enter your email address and we'll send you a link to reset your password.
forgot-password-email-label = Email Address
forgot-password-email-placeholder = you@example.com
forgot-password-cancel = Cancel
forgot-password-submit = Send Reset Link
forgot-password-submitting = Sending...
forgot-password-error-default = Failed to send reset email. Please try again.
forgot-password-success-title = Check Your Email
forgot-password-success-body = If an account with that email exists, we've sent a password reset link to { $email }
forgot-password-success-important = Important:
forgot-password-success-tip-expiry = The link will expire in <strong>1 hour</strong>
forgot-password-success-tip-spam = Check your spam folder if you don't see it
forgot-password-success-tip-close = You can close this window now
forgot-password-success-done = Done

# Profile settings tabs (top-level navigation on the settings
# view). Page-title strings reuse the same labels and append
# "Settings" / locale-equivalent.
settings-tab-profile = Profile
settings-tab-appearance = Appearance
settings-tab-language = Language
settings-tab-notifications = Notifications
settings-tab-security = Security
settings-sidebar-heading = Settings
settings-subtitle = Manage your profile, preferences, and security settings
settings-loading-user = Loading User Settings...
settings-user-heading = User Settings
settings-section-suffix = Settings

# Dashboard greeting + subtitle. The English variety pool
# (HAL, Christmas, standard) stays in useDashboardGreeting for
# en-* locales because the personality reads as deliberate.
# Other locales render a single canonical greeting per period
# from these keys, falling through to en-US naturally.
dashboard-greeting-morning = Good morning, { $name }.
dashboard-greeting-afternoon = Good afternoon, { $name }.
dashboard-greeting-evening = Good evening, { $name }.
dashboard-greeting-late-night = Hello { $name }, it's getting late.
dashboard-subtitle = Welcome to your { $app } dashboard
dashboard-edit-button = Edit dashboard
dashboard-guest-fallback = Guest

# Empty-state copy across major list views. Admin-only views
# (audit log, email queue, suppressions, plugin registry) stay
# in English for now — they're operator surfaces with technical
# language that doesn't carry the same UX cost as customer-
# facing screens.
empty-documentation-grid-title = No documentation yet
empty-documentation-grid-description = Create your first documentation page to get started.
empty-documentation-index-title = Start your knowledge base
empty-documentation-index-description = Documentation pages are how your team captures runbooks, FAQs, and policies. Create the first page to get going.
empty-documentation-archived-title = No archived pages
empty-documentation-archived-description = Archived pages will appear here.
empty-documentation-trash-title = Trash is empty
empty-documentation-trash-description = Deleted pages will appear here.
empty-project-search-title = No projects found
empty-project-search-description = Try adjusting your search criteria
empty-project-available-title = No projects available
empty-project-available-description = Create a project to get started
empty-device-search-prompt-title = Search for devices
empty-device-search-prompt-description = Start typing to find devices by name, serial number, or user
empty-device-search-title = No devices found
empty-device-search-description = Try adjusting your search criteria
empty-users-default-title = No users found
empty-users-default-description = Invite users to get started
empty-users-search-title = No users match your search
empty-users-search-description = Try adjusting your search criteria
empty-devices-default-title = No devices found
empty-devices-default-description = Add your first device to get started
empty-devices-search-title = No devices match your search
empty-devices-search-description = Try adjusting your search or filters
empty-groups-title = No groups yet
empty-groups-description = Create your first group to organize users
empty-assignment-rules-title = No assignment rules yet
empty-assignment-rules-description = Create your first rule to automatically assign tickets
empty-webhooks-title = No webhooks
empty-webhooks-description = Create a webhook to send events to external services
empty-api-tokens-title = No API tokens
empty-api-tokens-description = Create an API token to enable programmatic access to the API
empty-categories-title = No categories yet
empty-categories-description = Create categories to organize tickets
empty-plugins-installed-title = No plugins installed
empty-plugins-installed-description = Plugins extend { $app } with custom integrations and features. Browse the registry for one-click installs.

# Persistent shell — strings that render on every page.
nav-group-work = Work
nav-group-resources = Resources
nav-dashboard = Dashboard
nav-tickets = Tickets
nav-cycles = Cycles
nav-projects = Projects
nav-devices = Devices
nav-assets = Assets
nav-users = Users
nav-documentation = Documentation
nav-inbox = Inbox
nav-collapse = Collapse
nav-search = Search
nav-more = More
nav-toggle-sidebar = Toggle sidebar
nav-secondary = Secondary navigation
user-menu-aria = User menu
user-menu-view-profile = View Profile
user-menu-account = Account
user-menu-administration = Administration
user-menu-sign-out = Sign out
user-menu-guest-name = Guest

# Tickets list — empty states + bulk-action bar + chrome.
ticket-list-empty-no-assigned-message = No tickets assigned to you.
ticket-list-empty-showing-all-active = Showing all active tickets instead.
ticket-list-empty-no-match-title = No tickets match.
ticket-list-empty-no-match-description = Remove some filters to see more.
ticket-list-empty-triage-clear-title = Triage is clear.
ticket-list-empty-triage-clear-description = New tickets awaiting categorisation will appear here.
ticket-list-empty-all-caught-up-title = All caught up.
ticket-list-empty-all-caught-up-description = You have no open tickets assigned to you.
ticket-list-empty-no-active-title = No active tickets.
ticket-list-empty-no-active-description = Every ticket has been resolved or cancelled.
ticket-list-empty-no-in-view-title = No tickets in this view.
ticket-list-empty-no-in-view-description = Adjust the view filter or pick a different view.
ticket-list-bulk-actions-aria = Bulk actions
ticket-list-bulk-status = Status
ticket-list-bulk-priority = Priority
ticket-list-bulk-assign = Assign
ticket-list-bulk-clear-title = Clear selection (Esc)
ticket-list-bulk-clear = Clear
ticket-list-row-density-aria = Row density
ticket-list-save-view-title = Save current state as a private view
ticket-list-recurring-title = Recurring ticket
ticket-list-sla-breached-title = SLA breached

# Ticket detail — sidebar properties, section headers, danger zone.
ticket-detail-reconnecting-title = Reconnecting to live updates
ticket-detail-connecting = Connecting...
ticket-detail-more-actions = More actions
ticket-detail-section-details = Ticket Details
ticket-detail-section-notes = Ticket Notes
ticket-detail-section-comments = Comments and Attachments
ticket-detail-prop-title = Title
ticket-detail-prop-requester = Requester
ticket-detail-prop-assignee = Assignee
ticket-detail-prop-status = Status
ticket-detail-prop-priority = Priority
ticket-detail-prop-category = Category
ticket-detail-prop-created = Created
ticket-detail-prop-last-modified = Last Modified
ticket-detail-delete-title = Delete ticket
ticket-detail-delete-confirm-heading = Delete this ticket?
ticket-detail-delete-confirm-body = This action cannot be undone. The ticket and its history will be removed.
ticket-detail-delete-cancel = Cancel
ticket-detail-delete-confirm = Delete

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

# First-run admin onboarding (OnboardingView). Bootstrap token,
# admin form, fallback / completion screens, migration hint.
onboarding-welcome-title = Welcome to Nosdesk
onboarding-welcome-subtitle = Let's get started by creating your administrator account
onboarding-error-setup-status = Failed to verify setup status. Please try again.
onboarding-success-logging-in = Admin account created. Logging you in...
onboarding-success-fallback = Account created successfully. Please log in with your credentials.
onboarding-success-fallback-redirect = Account created successfully. Please log in.
onboarding-error-setup-failed = Setup failed. Please try again.
onboarding-error-unexpected = An unexpected error occurred. Please try again.
onboarding-validation-token = Bootstrap token is required
onboarding-validation-name = Administrator name is required
onboarding-validation-email = Email address is required
onboarding-validation-email-format = Please enter a valid email address
onboarding-validation-password-length = Password must be at least 8 characters long
onboarding-validation-password-mismatch = Passwords do not match
onboarding-token-label = Bootstrap Token
onboarding-token-placeholder = Paste the one-shot token from the server
onboarding-token-hint = Check the server startup logs for a setup URL, or retrieve manually with
onboarding-name-label = Administrator Name
onboarding-name-placeholder = Enter your full name
onboarding-email-label = Email Address
onboarding-email-placeholder = Enter your email address
onboarding-password-label = Password
onboarding-password-placeholder = Choose a secure password (8+ characters)
onboarding-confirm-password-label = Confirm Password
onboarding-confirm-password-placeholder = Confirm your password
onboarding-submit = Create Administrator Account
onboarding-submit-loading = Creating Administrator...
onboarding-progress-title = Setting up your account
onboarding-progress-subtitle = This will only take a moment...
onboarding-complete-title = Welcome to Nosdesk
onboarding-complete-subtitle = Your administrator account is ready.
onboarding-migration-title = Migrating from another Nosdesk instance?
onboarding-migration-body-prefix = Create an admin here, then run
onboarding-migration-body-suffix = on the host. The restore replaces the admin with the imported users.
onboarding-security-title = Security Notice
onboarding-security-body = This creates the first administrator account for your Nosdesk installation. Choose a strong password; this account will have full system access.

# MFA setup wizard (MFASetupView). Method-choice screen, post-
# setup offer screens, navigation labels.
mfa-setup-header-default = Complete Your Account Setup
mfa-setup-header-offer = Add Another Method?
mfa-setup-header-additional = Add Backup Method
mfa-setup-subtitle-default = Your account type requires multi-factor authentication for security
mfa-setup-subtitle-choose = Choose your preferred authentication method
mfa-setup-subtitle-offer-passkey = Passkeys provide a faster, passwordless sign-in experience
mfa-setup-subtitle-offer-totp = An authenticator app provides a backup if you lose your passkey
mfa-setup-subtitle-passkey-additional = Set up a passkey for faster sign-in
mfa-setup-subtitle-totp-additional = Set up an authenticator app as a backup
mfa-setup-totp-name = Authenticator App
mfa-setup-totp-description = Use an app like Google Authenticator, Authy, or 1Password to generate time-based codes
mfa-setup-passkey-name = Passkey
mfa-setup-passkey-description = Use biometrics like Face ID, Touch ID, or a hardware security key for passwordless login
mfa-setup-which-title = Which should I choose?
mfa-setup-which-passkey-label = Passkeys
mfa-setup-which-passkey-body = are more secure and convenient, just use your fingerprint or face.
mfa-setup-which-totp-label = Authenticator apps
mfa-setup-which-totp-body = work on any device and don't require biometrics.
mfa-setup-totp-success-title = Authenticator App Set Up!
mfa-setup-totp-success-body = Would you also like to add a passkey for faster, passwordless sign-in?
mfa-setup-passkey-success-title = Passkey Created!
mfa-setup-passkey-success-body = Would you also like to set up an authenticator app as a backup method?
mfa-setup-add-passkey-title = Add a Passkey
mfa-setup-add-passkey-description = Use Face ID, Touch ID, or a security key
mfa-setup-add-totp-title = Set Up Authenticator App
mfa-setup-add-totp-description = Use as a backup if you lose access to your passkey
mfa-setup-skip-now = Skip for now
mfa-setup-back-to-login = Back to Login
mfa-setup-back-skip = Skip
mfa-setup-back-different = Choose Different Method
mfa-setup-error-session-expired = Session expired. Please log in again to set up MFA.
mfa-setup-error-invalid-access = Invalid access. Redirecting to login...

# Password reset (PasswordResetView). Reached from the email link;
# form takes a new password + confirmation, then routes to login.
password-reset-title = Reset Your Password
password-reset-subtitle = Enter your new password below
password-reset-success-title = Password Reset Complete!
password-reset-success-body = Your password has been successfully updated. You can now log in with your new password.
password-reset-success-cta = Go to Login
password-reset-field-new = New Password
password-reset-field-new-placeholder = Enter new password
password-reset-field-confirm = Confirm New Password
password-reset-field-confirm-placeholder = Confirm new password
password-reset-req-length = At least 8 characters
password-reset-match-yes = Passwords match
password-reset-match-no = Passwords do not match
password-reset-submit = Reset Password
password-reset-submit-loading = Resetting Password...
password-reset-back-to-login = Back to Login
password-reset-error-no-token = Invalid or missing reset token. Please request a new password reset.
password-reset-error-failed = Failed to reset password. The link may have expired.

# Invitation / guest-ticket accept (AcceptInvitationView). Same
# form serves invitations and guest-ticket confirmations; copy
# pivots on `context`.
accept-invitation-heading-validating = Just a moment…
accept-invitation-heading-guest = Confirm your ticket submission
accept-invitation-heading-welcome = Welcome to { $app }
accept-invitation-subheading-validating = Verifying your link.
accept-invitation-subheading-guest = Set a password to release your ticket.
accept-invitation-subheading-invitation = Finish setting up your account.
accept-invitation-checking = Checking your link…
accept-invitation-invalid-title-guest = This confirmation link is no longer valid
accept-invitation-invalid-title-invitation = Invitation invalid
accept-invitation-go-to-signin = Go to sign in
accept-invitation-activating-title-guest = Releasing your ticket…
accept-invitation-activating-title-invitation = Activating your account…
accept-invitation-signing-in = Signing you in…
accept-invitation-success-title-guest = You're all set
accept-invitation-success-title-invitation = Welcome to { $app }
accept-invitation-manual-login = Please sign in with the password you just set.
accept-invitation-password-label = Password
accept-invitation-password-placeholder = At least 8 characters
accept-invitation-confirm-label = Confirm password
accept-invitation-confirm-placeholder = Enter it again
accept-invitation-req-length = At least 8 characters
accept-invitation-match-yes = Passwords match
accept-invitation-match-no = Passwords do not match
accept-invitation-show-password = Show password
accept-invitation-hide-password = Hide password
accept-invitation-submit-guest = Confirm & release ticket
accept-invitation-submit-loading-guest = Confirming…
accept-invitation-submit-invitation = Activate account
accept-invitation-submit-loading-invitation = Activating…
accept-invitation-back-to-signin = Back to sign in
accept-invitation-error-missing-token = Invalid or missing confirmation link.
accept-invitation-error-default = This link is invalid or has expired.
accept-invitation-error-validation-failed = Failed to validate link. Please try again later.
accept-invitation-error-submit = Failed to complete confirmation. The link may have expired.

# Admin: audit log (AuditLogView). Forensic record of who changed
# what across audited tier-1 tables.
admin-audit-title = Audit log
admin-audit-description = Forensic record of who changed what across audited entities. Defaults to the last 7 days and the most recent 50 entries; refine with the filters below.
admin-audit-filter-entity = Entity
admin-audit-filter-any = Any
admin-audit-filter-entity-id = Entity ID
admin-audit-filter-entity-id-placeholder = e.g. 42
admin-audit-filter-actor = Actor UUID
admin-audit-filter-actor-placeholder = e.g. 0192…
admin-audit-clear-filters = Clear filters
admin-audit-empty-title = No audit entries
admin-audit-empty-description = Either no audited entities have changed in the selected window, or the filters exclude every row.
admin-audit-by = by
admin-audit-corr = corr
admin-audit-diff-field = Field
admin-audit-diff-old = Old
admin-audit-diff-new = New
admin-audit-no-diff = No field-level diff for this entry.
admin-audit-op-created = Created
admin-audit-op-updated = Updated
admin-audit-op-deleted = Deleted
admin-audit-actor-system = system
admin-audit-load-more = Load more
admin-audit-loading-more = Loading…
admin-audit-error-load = Failed to load audit log
admin-audit-error-load-more = Failed to load more audit log entries

# Admin: email suppression list (EmailSuppressionsView).
admin-suppressions-title = Email suppression list
admin-suppressions-description = Addresses we won't attempt to deliver to. Hard bounces (5xx SMTP / 5.x.x enhanced status) land here automatically; add manually for compliance or complaint-driven blocks. Soft bounces (4xx, transient) never auto-suppress.
admin-suppressions-count-singular = suppression
admin-suppressions-count-plural = suppressions
admin-suppressions-add-title = Add a suppression
admin-suppressions-add-email-placeholder = user@example.com
admin-suppressions-add-note-placeholder = Optional note (compliance request, etc.)
admin-suppressions-adding = Adding…
admin-suppressions-add = Add
admin-suppressions-empty-title = No suppressions
admin-suppressions-empty-description = Hard-bounced recipients and manually-added addresses will appear here.
admin-suppressions-bounce-count-title = Bounced { $count } times
admin-suppressions-remove = Remove
admin-suppressions-confirm-title = Remove from suppression list?
admin-suppressions-confirm-message = Future sends to this address will be attempted normally. If the original failure was a hard bounce, they'll likely fail and re-suppress.
admin-suppressions-confirm-keep = Keep suppressed
admin-suppressions-load-more = Load more
admin-suppressions-loading-more = Loading…
admin-suppressions-error-load = Failed to load suppressions
admin-suppressions-error-load-more = Failed to load more
admin-suppressions-error-add = Failed to add suppression
admin-suppressions-error-remove = Failed to remove
admin-suppressions-reason-hard-bounce = hard bounce
admin-suppressions-reason-manual = manual

# Admin: outbound email queue (EmailQueueView).
admin-email-queue-title = Outbound email queue
admin-email-queue-description = Durable record of every reply we've tried to send. The worker drains pending rows every few seconds; failed sends retry with exponential backoff. Use this view to investigate why a notification didn't fire.
admin-email-queue-stat-pending = Pending
admin-email-queue-stat-oldest = Oldest: { $age }
admin-email-queue-stat-sent = Sent
admin-email-queue-stat-failed = Failed (retrying)
admin-email-queue-stat-dead = Dead (no retry)
admin-email-queue-filter-status = Status
admin-email-queue-filter-ticket = Ticket ID
admin-email-queue-filter-ticket-placeholder = 42
admin-email-queue-filter-domain = Recipient domain
admin-email-queue-filter-domain-placeholder = example.com
admin-email-queue-clear-filters = Clear filters
admin-email-queue-status-pending = pending
admin-email-queue-status-sending = sending
admin-email-queue-status-sent = sent
admin-email-queue-status-failed = failed
admin-email-queue-status-dead = dead
admin-email-queue-status-suppressed = suppressed
admin-email-queue-empty-title = No outbound emails
admin-email-queue-empty-description = Either no replies have been sent recently, or the filters exclude every row.
admin-email-queue-bounced = Bounced
admin-email-queue-bounced-with-diagnostic = Bounced: { $diagnostic }
admin-email-queue-bounced-no-diagnostic = Bounced (no upstream diagnostic captured)
admin-email-queue-attempts-title = { $count } attempt(s)
admin-email-queue-retry-now = Retry now
admin-email-queue-cancel = Cancel
admin-email-queue-details = Details
admin-email-queue-hide = Hide
admin-email-queue-field-recipient = Recipient
admin-email-queue-field-channel = Channel
admin-email-queue-field-ticket = Ticket
admin-email-queue-field-comment = Comment
admin-email-queue-field-next-attempt = Next attempt
admin-email-queue-field-sent-at = Sent at
admin-email-queue-field-failed-at = Failed at
admin-email-queue-field-smtp-code = SMTP code
admin-email-queue-field-last-error = Last error
admin-email-queue-field-bounced-at = Bounced at
admin-email-queue-field-bounce-recipient = Bounce recipient
admin-email-queue-field-bounce-reason = Bounce reason
admin-email-queue-load-more = Load more
admin-email-queue-loading-more = Loading…
admin-email-queue-confirm-title = Cancel queued email?
admin-email-queue-confirm-message = The email will be marked suppressed and will not be sent.
admin-email-queue-confirm-yes = Cancel send
admin-email-queue-confirm-no = Keep it
admin-email-queue-error-load = Failed to load email queue
admin-email-queue-error-load-more = Failed to load more queue entries
admin-email-queue-error-stats = Failed to load queue stats
admin-email-queue-error-retry = Retry failed
admin-email-queue-error-cancel = Cancel failed

# Admin: workflow states (WorkflowStatesView). Operators name
# their ticket states within fixed categories.
admin-workflow-states-title = Workflow
admin-workflow-states-description = Add named ticket states inside the standard workflow categories. Categories are fixed so SLA, dashboards, and automation keep working consistently across teams. New tickets land in the state marked as default.
admin-workflow-states-count-singular = state
admin-workflow-states-count-plural = states
admin-workflow-states-default-badge = Default
admin-workflow-states-make-default = Make default
admin-workflow-states-archive-title = Archive state
admin-workflow-states-archive-disabled-title = Cannot archive the default state
admin-workflow-states-archive-confirm = Archive "{ $name }"? Existing tickets will keep this state.
admin-workflow-states-empty-category = No states in this category.
admin-workflow-states-add-placeholder = Add state name
admin-workflow-states-add = Add
admin-workflow-states-error-name-required = Name is required
admin-workflow-states-error-load = Failed to load workflow states
admin-workflow-states-error-save = Failed to save state
admin-workflow-states-error-default = Failed to set default
admin-workflow-states-error-archive = Failed to archive state
admin-workflow-states-error-promote-first = Promote another state as default before archiving this one.
admin-workflow-states-error-create = Failed to create state
admin-workflow-states-saved = Saved
admin-workflow-states-default-flash = { $name } is now the default for new tickets
admin-workflow-states-archived-flash = { $name } archived
admin-workflow-states-added-flash = { $name } added to { $category }

# Admin chrome — sidebar header, search, breadcrumb, index page
# title and copy, group labels and 19 nav items (title + blurb).
admin-back-to-dashboard = Back to Dashboard
admin-heading = Administration
admin-search-placeholder = Search settings...
admin-search-empty = No settings match "{ $query }"
admin-clear-search = Clear search
admin-index-subtitle = Manage your system settings, integrations, and workspace configuration

admin-nav-group-tickets = Tickets & Workflow
admin-nav-group-integrations = Integrations
admin-nav-group-compliance = Compliance
admin-nav-group-appearance = Appearance & Notifications
admin-nav-group-system = System

admin-nav-groups-title = Groups
admin-nav-groups-description = Manage user groups and memberships
admin-nav-categories-title = Categories
admin-nav-categories-description = Configure ticket categories and group visibility
admin-nav-assignment-rules-title = Assignment Rules
admin-nav-assignment-rules-description = Configure automatic ticket assignment based on rules
admin-nav-workflow-title = Workflow
admin-nav-workflow-description = Add named ticket states inside the standard workflow categories
admin-nav-sla-title = SLA
admin-nav-sla-description = Service-level policies and working-hours calendars
admin-nav-api-tokens-title = API Tokens
admin-nav-api-tokens-description = Manage API tokens for programmatic access
admin-nav-webhooks-title = Webhooks
admin-nav-webhooks-description = Configure webhooks to send events to external services
admin-nav-plugins-title = Plugins
admin-nav-plugins-description = Manage installed plugins and integrations
admin-nav-data-import-title = Data Import
admin-nav-data-import-description = Import data from Intune, CSV files, and other sources
admin-nav-channels-email-title = Email Ingestion
admin-nav-channels-email-description = Poll a support mailbox over IMAP and turn messages into tickets
admin-nav-email-queue-title = Email Queue
admin-nav-email-queue-description = Outbound email durable queue: status, retries, bounces, and per-row actions
admin-nav-email-suppressions-title = Email Suppressions
admin-nav-email-suppressions-description = Addresses blocked from outbound delivery, auto-populated by hard bounces
admin-nav-audit-log-title = Audit Log
admin-nav-audit-log-description = Forensic record of who changed what, drawn from per-table triggers
admin-nav-branding-title = Branding
admin-nav-branding-description = Customize the appearance and branding of the application
admin-nav-email-settings-title = Email Configuration
admin-nav-email-settings-description = Configure SMTP settings and send test emails
admin-nav-guest-access-title = Guest Access
admin-nav-guest-access-description = Control what unauthenticated visitors can see and submit
admin-nav-auth-providers-title = Authentication Providers
admin-nav-auth-providers-description = Configure SSO, Microsoft Entra, and local authentication
admin-nav-search-title = Search
admin-nav-search-description = Manage the search index and view indexing statistics
admin-nav-system-settings-title = System Settings
admin-nav-system-settings-description = Manage storage, cleanup stale files, and system maintenance
admin-nav-backup-restore-title = Backup & Restore
admin-nav-backup-restore-description = Export and restore system data and attachments

# Admin: System Settings (SystemSettingsView). Storage cleanup
# and system info card.
admin-system-title = System Settings
admin-system-storage-title = Storage Management
admin-system-storage-description = Remove old user profile images and avatars that are no longer needed to free up disk space.
admin-system-storage-clean = Clean Up
admin-system-storage-cleaning = Cleaning...
admin-system-storage-confirm-title = Clean up stale images?
admin-system-storage-confirm-message = This action cannot be undone.
admin-system-storage-confirm-label = Clean up
admin-system-cleanup-success = Cleanup Completed
admin-system-cleanup-failed = Cleanup Failed
admin-system-cleanup-stat-avatars = Avatars:
admin-system-cleanup-stat-banners = Banners:
admin-system-cleanup-stat-thumbnails = Thumbnails:
admin-system-cleanup-stat-checked = Checked:
admin-system-cleanup-stat-errors = Errors:
admin-system-cleanup-view-errors = View Errors ({ $count })
admin-system-cleanup-error-unexpected = An unexpected error occurred while cleaning up images

# Admin: Search Index Management (SearchManagementView).
admin-search-mgmt-title = Search Index Management
admin-search-mgmt-description = Manage the full-text search index for tickets, documentation, devices, and users.
admin-search-mgmt-stats-title = Index Statistics
admin-search-mgmt-refresh = Refresh
admin-search-mgmt-total-documents = Total Documents
admin-search-mgmt-index-size = Index Size
admin-search-mgmt-status = Status
admin-search-mgmt-status-rebuilding = Rebuilding
admin-search-mgmt-status-ready = Ready
admin-search-mgmt-entity-types = Entity Types
admin-search-mgmt-stats-error = Failed to fetch search index statistics
admin-search-mgmt-rebuild-title = Rebuild Search Index
admin-search-mgmt-rebuild-description = Rebuilds the entire search index from the database. Re-indexes all tickets, comments, documentation pages, attachments, devices, and users. Use this if search results are missing or outdated.
admin-search-mgmt-rebuild = Rebuild Index
admin-search-mgmt-rebuilding = Rebuilding...
admin-search-mgmt-rebuild-success = Index Rebuilt Successfully
admin-search-mgmt-rebuild-failed = Rebuild Failed
admin-search-mgmt-rebuild-stat-tickets = Tickets:
admin-search-mgmt-rebuild-stat-comments = Comments:
admin-search-mgmt-rebuild-stat-docs = Docs:
admin-search-mgmt-rebuild-stat-attachments = Attachments:
admin-search-mgmt-rebuild-stat-devices = Devices:
admin-search-mgmt-rebuild-stat-users = Users:
admin-search-mgmt-rebuild-stat-total = Total:
admin-search-mgmt-rebuild-confirm-title = Rebuild the search index?
admin-search-mgmt-rebuild-confirm-message = This may take a few moments depending on the amount of data.
admin-search-mgmt-rebuild-confirm-label = Rebuild
admin-search-mgmt-rebuild-error-unexpected = An unexpected error occurred while rebuilding the index

# Admin: Email Configuration (EmailSettingsView). Read-only view
# of SMTP env vars plus a "send test" form.
admin-email-settings-title = Email Configuration
admin-email-settings-description = View email configuration status and send test emails. Email settings are configured via environment variables.
admin-email-settings-env-notice-prefix = Email settings are configured through environment variables in your
admin-email-settings-env-notice-suffix = file or Docker environment. Use the "Send Test Email" feature to verify your configuration is working correctly.
admin-email-settings-loading = Loading email configuration...
admin-email-settings-service = SMTP Email Service
admin-email-settings-configured = Configured
admin-email-settings-not-configured = Not Configured
admin-email-settings-enabled = Enabled
admin-email-settings-server = Server
admin-email-settings-username = Username
admin-email-settings-from-address = From Address
admin-email-settings-password = Password
admin-email-settings-password-not-set = Not Set
admin-email-settings-env-vars-label = Env:
admin-email-settings-test-send = Send test:
admin-email-settings-test-placeholder = recipient@example.com
admin-email-settings-test-send-button = Send
admin-email-settings-test-sending = Sending...
admin-email-settings-empty-title = Email is not configured
admin-email-settings-empty-description = Configure email settings in your environment variables to enable email functionality
admin-email-settings-error-load = Failed to load email configuration
admin-email-settings-error-no-address = Please enter an email address
admin-email-settings-error-bad-address = Please enter a valid email address
admin-email-settings-test-success = Test email sent successfully
admin-email-settings-error-test = Failed to send test email

# Admin: Guest Access (GuestAccessSettingsView). Public-feature
# toggles and public-form submission policy.
admin-guest-title = Guest Access
admin-guest-description = Control what unauthenticated visitors can see and submit. All features are disabled by default.
admin-guest-loading = Loading guest settings...
admin-guest-features-title = Public Features
admin-guest-toggle-tickets-label = Accept guest ticket submissions
admin-guest-toggle-tickets-description = Shows a public ticket form at /submit-ticket.
admin-guest-toggle-lookup-label = Guest ticket status lookup
admin-guest-toggle-lookup-description = Lets guests check status via a private link returned on submit.
admin-guest-toggle-public-docs-label = Public documentation
admin-guest-toggle-public-docs-description = Exposes pages marked 'public' at /docs without requiring login.
admin-guest-toggle-kb-search-label = Public knowledge base search
admin-guest-toggle-kb-search-description = Search over public documentation. Requires 'Public documentation' on.
admin-guest-toggle-help-label = Self-service help page
admin-guest-toggle-help-description = Static /help page with links to password reset and ticket submission.
admin-guest-submissions-title = Guest Ticket Submissions
admin-guest-submissions-description = Behavior for tickets submitted through the public form.
admin-guest-toggle-email-verification-label = Require email confirmation
admin-guest-toggle-email-verification-description = Hold submissions until the requester confirms via email. Also gives them portal access.
admin-guest-toggle-attachments-label = Allow attachments
admin-guest-toggle-attachments-description = Submitters can attach images, PDFs, and text/log files (≤10MB each, up to 5 per ticket).
admin-guest-default-priority-label = Default priority
admin-guest-default-priority-hint = Applied to every guest submission. Techs can re-triage after.
admin-guest-priority-low = Low
admin-guest-priority-medium = Medium
admin-guest-priority-high = High
admin-guest-intro-message-label = Intro message
admin-guest-intro-message-optional = (optional)
admin-guest-intro-message-placeholder = e.g. For urgent outages call 555-1234. Check our docs first at /docs.
admin-guest-intro-message-hint = Shown above the public submit form. Plain text, line breaks preserved.
admin-guest-intro-message-count = { $count } / 500
admin-guest-rate-limit-label = Rate limit
admin-guest-rate-limit-suffix = per IP / hour
admin-guest-rate-limit-hint = Lower this if you see spam from shared IPs.
admin-guest-unsaved = Unsaved changes
admin-guest-save = Save Settings
admin-guest-saving = Saving...
admin-guest-error-load = Failed to load guest settings
admin-guest-error-save = Failed to save guest settings
admin-guest-saved = Guest access settings saved

# Admin: Data Import hub (DataImportView). Landing page with
# tiles for the available import sources.
admin-data-import-title = Data Import
admin-data-import-description = Import and synchronize data from external sources
admin-data-import-notice = Data imports may trigger notifications to affected users. Existing records are updated based on matching IDs.
admin-data-import-status-available = Available
admin-data-import-status-coming-soon = Coming Soon
admin-data-import-status-beta = Beta
admin-data-import-microsoft-title = Microsoft Graph
admin-data-import-microsoft-description = Import data from Microsoft 365, including Azure AD, Intune, and other Microsoft services
admin-data-import-csv-title = CSV Import
admin-data-import-csv-description = Import data from CSV files, including devices, users, and other resources
admin-data-import-api-title = API Integrations
admin-data-import-api-description = Connect to third-party APIs to import and synchronize data
admin-data-import-ad-title = Active Directory
admin-data-import-ad-description = Import data from on-premises Active Directory servers

# Admin: Authentication Providers (AuthProvidersView).
admin-auth-providers-title = Authentication Providers
admin-auth-providers-env-notice-prefix = Authentication providers are configured through environment variables in your
admin-auth-providers-env-notice-suffix = file. Use the "Validate Config" button to check if each provider is properly configured.
admin-auth-providers-loading = Loading providers...
admin-auth-providers-default-badge = Default
admin-auth-providers-configured = Configured
admin-auth-providers-not-configured = Not Configured
admin-auth-providers-enabled = Enabled
admin-auth-providers-client-id = Client ID
admin-auth-providers-tenant-id = Tenant ID
admin-auth-providers-redirect-uri = Redirect URI
admin-auth-providers-secret = Secret
admin-auth-providers-secret-not-set = Not Set
admin-auth-providers-env-label = Env:
admin-auth-providers-empty-title = No authentication providers found
admin-auth-providers-empty-description = Configure authentication providers in your environment variables
admin-auth-providers-error-load = Failed to load authentication providers
admin-auth-providers-error-validate = Configuration validation failed

# Admin: API Tokens (ApiTokensView). Create, view, and revoke
# tokens that act-as a specific user.
admin-api-tokens-title = API Tokens
admin-api-tokens-description = Manage API tokens for programmatic access
admin-api-tokens-create = Create Token
admin-api-tokens-create-short = Create
admin-api-tokens-loading = Loading tokens...
admin-api-tokens-active-heading = Active Tokens
admin-api-tokens-revoked-heading = Revoked Tokens
admin-api-tokens-user-prefix = User:
admin-api-tokens-created-prefix = Created { $when }
admin-api-tokens-expires-prefix = Expires { $when }
admin-api-tokens-no-expiration = No expiration
admin-api-tokens-last-used-label = Last used:
admin-api-tokens-last-used-never = Never
admin-api-tokens-revoked-prefix = Revoked { $when }
admin-api-tokens-revoke-title = Revoke token
admin-api-tokens-error-load = Failed to load API tokens
admin-api-tokens-error-create = Failed to create token
admin-api-tokens-error-revoke = Failed to revoke token
admin-api-tokens-error-name-required = Token name is required
admin-api-tokens-error-user-required = Please select a user
admin-api-tokens-revoke-success = Token revoked successfully
admin-api-tokens-modal-create-title = Create API Token
admin-api-tokens-modal-name-label = Token Name
admin-api-tokens-modal-name-placeholder = e.g., CI/CD Pipeline
admin-api-tokens-modal-name-hint = A descriptive name to identify this token
admin-api-tokens-modal-user-label = User (acts as)
admin-api-tokens-modal-user-placeholder = Select a user...
admin-api-tokens-modal-user-hint = The token will have the same permissions as this user
admin-api-tokens-modal-expiration-label = Expiration
admin-api-tokens-modal-no-expiration-label = No expiration
admin-api-tokens-modal-expires-days-suffix = days
admin-api-tokens-modal-expires-hint = Token will expire after { $days } days
admin-api-tokens-modal-no-expiration-warning = Tokens without expiration are less secure
admin-api-tokens-modal-cancel = Cancel
admin-api-tokens-modal-creating = Creating...
admin-api-tokens-created-title = Token Created
admin-api-tokens-created-warning = Copy this token now, it won't be shown again!
admin-api-tokens-copied = Copied!
admin-api-tokens-copy-title = Copy to clipboard
admin-api-tokens-bearer-hint-prefix = Use this token with the
admin-api-tokens-bearer-hint-suffix = header
admin-api-tokens-done = Done
admin-api-tokens-revoke-modal-title = Revoke Token
admin-api-tokens-revoke-confirm-message = Are you sure you want to revoke the token "{ $name }"?
admin-api-tokens-revoke-warning = This action cannot be undone. Any systems using this token will lose access.
admin-api-tokens-revoking = Revoking...

# Admin: SLA (SlaAdminView). Working calendars and SLA policies
# side by side; both have inline create-forms.
admin-sla-title = SLA
admin-sla-description = Working calendars and SLA policies feed the per-ticket SLA pill.
admin-sla-loading = Loading…
admin-sla-error-load = Failed to load SLA config
admin-sla-error-create = Create failed
admin-sla-error-delete = Delete failed
admin-sla-error-update = Update failed
admin-sla-calendars-heading = Working calendars
admin-sla-policies-heading = SLA policies
admin-sla-col-name = Name
admin-sla-col-tz = TZ
admin-sla-col-default = Default
admin-sla-col-response = Response
admin-sla-col-resolution = Resolution
admin-sla-col-calendar = Calendar
admin-sla-default-badge = Default
admin-sla-set-default = Set default
admin-sla-delete = Delete
admin-sla-calendar-delete-confirm = Delete this calendar? Policies pointing at it will need a new calendar.
admin-sla-policy-delete-confirm = Delete this policy? Tickets that currently match it will lose their SLA pill until another policy matches them. This cannot be undone.
admin-sla-new-calendar-heading = New calendar
admin-sla-new-policy-heading = New policy
admin-sla-field-name = Name
admin-sla-field-tz = Timezone
admin-sla-field-calendar = Calendar
admin-sla-field-response = Response (minutes)
admin-sla-field-resolution = Resolution (minutes)
admin-sla-field-priority = Priority filter
admin-sla-placeholder-name = EU support hours
admin-sla-placeholder-tz = Europe/London
admin-sla-policy-name-placeholder = Critical incidents
admin-sla-schedule-hint = Schedule defaults to Mon-Fri 9-17. Edit by hand or expand here later.
admin-sla-priority-any = Any
admin-sla-priority-low = low
admin-sla-priority-medium = medium
admin-sla-priority-high = high
admin-sla-workspace-default = Workspace default
admin-sla-create = Create

# Admin: Branding (BrandingSettingsView). Custom app name, primary
# color, logo (dark + optional light) and favicon uploads.
admin-branding-title = Branding
admin-branding-description = Customize the appearance and branding of the application.
admin-branding-loading = Loading branding configuration...
admin-branding-general-heading = General Settings
admin-branding-app-name-label = Application Name
admin-branding-app-name-placeholder = Nosdesk
admin-branding-app-name-hint = This name appears in the header and browser tab
admin-branding-primary-color-label = Primary Color
admin-branding-primary-color-hint = Hex color code for accent elements (e.g., #2C80FF)
admin-branding-save = Save Settings
admin-branding-saving = Saving...
admin-branding-logo-heading = Logo
admin-branding-logo-dark-label = Dark Theme Logo
admin-branding-logo-light-label = Light Theme Logo (Optional)
admin-branding-logo-upload = Upload Logo
admin-branding-logo-uploading = Uploading...
admin-branding-logo-remove = Remove
admin-branding-logo-formats = PNG, SVG, JPEG, or WebP. Max 2MB.
admin-branding-logo-light-hint = Used when light theme is active. Falls back to main logo.
admin-branding-favicon-heading = Favicon
admin-branding-favicon-upload = Upload Favicon
admin-branding-favicon-uploading = Uploading...
admin-branding-favicon-formats = ICO, PNG, or SVG. Recommended size: 32x32 or 64x64 pixels.
admin-branding-preview-heading = Preview
admin-branding-primary-color-preview = Primary Color
admin-branding-configured = Custom branding configured
admin-branding-success-saved = Branding settings saved successfully
admin-branding-success-logo = Logo uploaded successfully
admin-branding-success-logo-light = Light theme logo uploaded successfully
admin-branding-success-favicon = Favicon uploaded successfully
admin-branding-success-removed = { $asset } removed successfully
admin-branding-error-load = Failed to load branding configuration
admin-branding-error-save = Failed to save branding settings
admin-branding-error-invalid-file = Invalid file
admin-branding-error-upload-logo = Failed to upload logo
admin-branding-error-upload-logo-light = Failed to upload light theme logo
admin-branding-error-upload-favicon = Failed to upload favicon
admin-branding-error-delete = Failed to delete { $asset }
admin-branding-asset-logo = Logo
admin-branding-asset-logo-light = Light theme logo
admin-branding-asset-favicon = Favicon
admin-branding-confirm-title = Remove { $asset }?
admin-branding-confirm-message = This removes the uploaded image. You can re-upload at any time, but the previous file is not recoverable.
admin-branding-confirm-remove = Remove

# Admin: Backup & Restore (BackupRestoreView). Create backup +
# optional encrypted sensitive data, recent backups list,
# documentation export, and restore-from-zip flow.
admin-backup-title = Backup & Restore
admin-backup-description = Export and restore system data and attachments
admin-backup-create-heading = Create Backup
admin-backup-create-description = Export all system data and attachments to a ZIP archive
admin-backup-include-sensitive-label = Include sensitive data
admin-backup-include-sensitive-description = Includes passwords, MFA secrets, and authentication tokens (encrypted with password)
admin-backup-encryption-warning = Sensitive data will be encrypted. If you lose the password, the data cannot be recovered.
admin-backup-encryption-password-label = Encryption Password
admin-backup-encryption-password-placeholder = Enter encryption password
admin-backup-confirm-password-label = Confirm Password
admin-backup-confirm-password-placeholder = Confirm encryption password
admin-backup-passwords-no-match = Passwords do not match
admin-backup-create-button = Create Backup
admin-backup-creating = Creating Backup...
admin-backup-recent-heading = Recent Backups
admin-backup-refresh = Refresh
admin-backup-empty = No backups yet. Create your first backup above.
admin-backup-encrypted-badge = Encrypted
admin-backup-creating-status = Creating...
admin-backup-download-title = Download
admin-backup-delete-title = Delete
admin-backup-docs-heading = Export Documentation to Markdown
admin-backup-docs-description = Export all documentation pages as markdown files in a ZIP archive
admin-backup-docs-export = Export as Markdown
admin-backup-docs-exporting = Exporting { $current }/{ $total }...
admin-backup-docs-preparing = Preparing...
admin-backup-docs-error = Failed to export documentation. Please check the console for details.
admin-backup-restore-heading = Restore from Backup
admin-backup-restore-description = Upload a backup file to restore system data and attachments
admin-backup-restore-dnd = Drag and drop a backup file here, or
admin-backup-restore-browse = browse to select a file
admin-backup-details-heading = Backup Details
admin-backup-detail-created = Created:
admin-backup-detail-version = Version:
admin-backup-detail-files = Files:
admin-backup-detail-size = Size:
admin-backup-detail-tables = Tables:
admin-backup-warnings-heading = Warnings
admin-backup-decryption-password-label = Decryption Password
admin-backup-decryption-password-placeholder = Enter backup encryption password
admin-backup-restore-warning = Restoring will replace existing files. This action cannot be undone.
admin-backup-restore-button = Restore Files
admin-backup-restoring = Restoring...
admin-backup-cancel = Cancel
admin-backup-restore-not-zip = Please select a .zip backup file
admin-backup-upload-error = Failed to upload backup file
admin-backup-restore-success = Restore completed: { $files } files restored. { $message }
admin-backup-restore-error = Restore failed. Please check the console for details.
admin-backup-delete-confirm-title = Delete this backup?
admin-backup-delete-confirm-message = The backup file will be permanently removed.
admin-backup-delete-confirm-label = Delete

# Admin: Assignment Rules (AssignmentRulesView).
admin-assignment-rules-title = Assignment Rules
admin-assignment-rules-description = Configure automatic ticket assignment based on rules
admin-assignment-rules-new = New Rule
admin-assignment-rules-info = Rules are evaluated in priority order (top to bottom). The first matching rule wins. Tickets with an existing assignee are not auto-assigned.
admin-assignment-rules-loading = Loading rules...
admin-assignment-rules-active = Active
admin-assignment-rules-inactive = Inactive
admin-assignment-rules-target-none = Not configured
admin-assignment-rules-trigger-both = Both triggers
admin-assignment-rules-trigger-create = On create
admin-assignment-rules-trigger-category = On category change
admin-assignment-rules-trigger-none = No triggers
admin-assignment-rules-assigned-count = { $count } assigned
admin-assignment-rules-move-up = Move up (higher priority)
admin-assignment-rules-move-down = Move down (lower priority)
admin-assignment-rules-toggle-deactivate = Deactivate rule
admin-assignment-rules-toggle-activate = Activate rule
admin-assignment-rules-edit = Edit rule
admin-assignment-rules-delete = Delete rule
admin-assignment-rules-create-action = Create Rule
admin-assignment-rules-error-load = Failed to load assignment rules
admin-assignment-rules-error-name = Rule name is required
admin-assignment-rules-error-user = Please select a target user
admin-assignment-rules-error-group = Please select a target group
admin-assignment-rules-error-save = Failed to save rule
admin-assignment-rules-error-update = Failed to update rule
admin-assignment-rules-error-delete = Failed to delete rule
admin-assignment-rules-error-reorder = Failed to reorder rules
admin-assignment-rules-success-create = Rule created successfully
admin-assignment-rules-success-update = Rule updated successfully
admin-assignment-rules-success-delete = Rule deleted successfully
admin-assignment-rules-method-direct-label = Direct User
admin-assignment-rules-method-direct-description = Assign directly to a specific user
admin-assignment-rules-method-round-robin-label = Round-Robin (Group)
admin-assignment-rules-method-round-robin-description = Rotate assignment among group members evenly
admin-assignment-rules-method-random-label = Random (Group)
admin-assignment-rules-method-random-description = Randomly select a group member for each ticket
admin-assignment-rules-method-queue-label = Group Queue
admin-assignment-rules-method-queue-description = Assign to group queue (users claim tickets)
admin-assignment-rules-modal-create-title = Create Assignment Rule
admin-assignment-rules-modal-edit-title = Edit Assignment Rule
admin-assignment-rules-modal-name-label = Rule Name
admin-assignment-rules-modal-name-placeholder = e.g., IT Support Round-Robin
admin-assignment-rules-modal-description-label = Description (optional)
admin-assignment-rules-modal-description-placeholder = Describe what this rule does...
admin-assignment-rules-modal-method-label = Assignment Method
admin-assignment-rules-modal-user-label = Target User
admin-assignment-rules-modal-user-placeholder = Select a user...
admin-assignment-rules-modal-group-label = Target Group
admin-assignment-rules-modal-group-placeholder = Select a group...
admin-assignment-rules-modal-group-members = { $count } members
admin-assignment-rules-modal-category-label = Category Filter (optional)
admin-assignment-rules-modal-category-all = All categories
admin-assignment-rules-modal-category-hint = Only assign tickets with this category (leave empty for all)
admin-assignment-rules-modal-triggers-label = Triggers
admin-assignment-rules-modal-trigger-create-label = When a ticket is created
admin-assignment-rules-modal-trigger-category-label = When a ticket's category changes
admin-assignment-rules-modal-active-label = Rule is active
admin-assignment-rules-modal-cancel = Cancel
admin-assignment-rules-modal-saving = Saving...
admin-assignment-rules-modal-update = Update Rule
admin-assignment-rules-modal-create = Create Rule
admin-assignment-rules-delete-title = Delete Assignment Rule
admin-assignment-rules-delete-message = Are you sure you want to delete the rule "{ $name }"? This action cannot be undone.
admin-assignment-rules-delete-cancel = Cancel
admin-assignment-rules-delete-confirm = Delete
admin-assignment-rules-deleting = Deleting...

# Admin: Categories (CategoriesManagementView).
admin-categories-title = Categories
admin-categories-description = Manage ticket categories and group visibility
admin-categories-new = New Category
admin-categories-info = Categories with no group restrictions are visible to all users. Assign groups to restrict visibility.
admin-categories-loading = Loading categories...
admin-categories-search-placeholder = Search categories...
admin-categories-filter-all = All Categories
admin-categories-filter-active = Active Only
admin-categories-filter-inactive = Inactive Only
admin-categories-filter-public = Public Only
admin-categories-filter-restricted = Restricted Only
admin-categories-sort-custom = Custom Order
admin-categories-sort-name = Name
admin-categories-sort-ascending = Ascending
admin-categories-sort-descending = Descending
admin-categories-drag-handle = Drag to reorder
admin-categories-badge-public = Public
admin-categories-badge-groups = { $count ->
    [one] { $count } group
   *[other] { $count } groups
    }
admin-categories-badge-inactive = Inactive
admin-categories-groups-more = +{ $count } more
admin-categories-action-deactivate = Deactivate
admin-categories-action-activate = Activate
admin-categories-action-edit = Edit category
admin-categories-action-delete = Delete category
admin-categories-no-search-results = No categories matching "{ $query }"
admin-categories-no-filter-results = No categories match the current filter
admin-categories-empty-action = Create Category
admin-categories-modal-create-title = Create Category
admin-categories-modal-edit-title = Edit Category
admin-categories-modal-name-label = Name
admin-categories-modal-name-placeholder = Enter category name
admin-categories-modal-description-label = Description
admin-categories-modal-description-placeholder = Optional description
admin-categories-modal-icon-label = Icon
admin-categories-modal-color-label = Color
admin-categories-modal-active-label = Active
admin-categories-modal-visibility-label = Visible to Groups
admin-categories-modal-visibility-hint = (leave empty for public)
admin-categories-modal-visibility-toggle-aria = Toggle visibility for { $name }
admin-categories-modal-group-members = { $count } members
admin-categories-modal-no-groups = No groups available.
admin-categories-modal-create-groups-link = Create groups
admin-categories-modal-create-groups-suffix = first.
admin-categories-modal-cancel = Cancel
admin-categories-modal-save = Save Changes
admin-categories-modal-create = Create Category
admin-categories-delete-title = Delete Category
admin-categories-delete-message = Are you sure you want to delete the category "{ $name }"? Tickets using this category will have their category cleared.
admin-categories-delete-cancel = Cancel
admin-categories-delete-confirm = Delete Category
admin-categories-error-name-required = Category name is required
admin-categories-error-load = Failed to load categories
admin-categories-error-reorder = Failed to reorder categories
admin-categories-error-save = Failed to save category
admin-categories-error-update = Failed to update category
admin-categories-error-delete = Failed to delete category
admin-categories-success-create = Category created successfully
admin-categories-success-update = Category updated successfully
admin-categories-success-delete = Category deleted successfully

# Admin: Email channels (ChannelsEmailSettingsView).
admin-channels-email-title = Email Ingestion
admin-channels-email-description = Poll a support mailbox over IMAP and turn inbound messages into tickets. Replies from techs are relayed back through the same thread.
admin-channels-email-loading = Loading channel...
admin-channels-email-status-heading = Status
admin-channels-email-status-subtitle = Live view of what the ingestion worker last did.
admin-channels-email-status-enabled = Enabled
admin-channels-email-status-disabled = Disabled
admin-channels-email-status-last-polled = Last polled
admin-channels-email-status-never = never
admin-channels-email-status-last-uid = Last seen UID
admin-channels-email-status-uid-validity = UIDVALIDITY
admin-channels-email-status-last-error = Last error
admin-channels-email-status-last-error-hint = The worker will keep retrying with exponential backoff. Fix the underlying issue and it'll clear on the next successful poll.
admin-channels-email-form-heading-edit = Configuration
admin-channels-email-form-heading-create = Connect a mailbox
admin-channels-email-form-subtitle = IMAP over TLS only. For self-hosted test servers with a self-signed cert, see the advanced toggle below.
admin-channels-email-toggle-enabled-label = Enabled
admin-channels-email-toggle-enabled-description = When off, the worker stops polling but stored config and credentials are preserved.
admin-channels-email-field-name-label = Display name
admin-channels-email-field-name-placeholder = e.g. Support Inbox
admin-channels-email-field-name-hint = Only shown in the admin UI. Customers never see it.
admin-channels-email-field-host-label = IMAP host
admin-channels-email-field-host-placeholder = imap.example.com
admin-channels-email-field-port-label = Port
admin-channels-email-field-port-hint = 993 for IMAPS. 143 requires STARTTLS (not supported yet).
admin-channels-email-field-username-label = Username
admin-channels-email-field-username-placeholder = support@example.com
admin-channels-email-field-mailbox-label = Mailbox
admin-channels-email-field-mailbox-placeholder = INBOX
admin-channels-email-field-mailbox-hint = Gmail users may want "[Gmail]/All Mail".
admin-channels-email-field-reply-domain-label = Reply domain
admin-channels-email-field-reply-domain-placeholder = example.com
admin-channels-email-field-reply-domain-hint = Used when we stamp Message-IDs on outbound replies so the customer's reply threads back to the same ticket. Usually the same domain as the username.
admin-channels-email-field-password-label = Password
admin-channels-email-field-password-keep-existing = (leave blank to keep existing)
admin-channels-email-field-password-placeholder-stored = •••••••••• (stored)
admin-channels-email-field-password-placeholder-new = App password or account password
admin-channels-email-remove-password = Remove stored password
admin-channels-email-removing-password = Removing...
admin-channels-email-advanced = Advanced
admin-channels-email-toggle-insecure-label = Skip TLS certificate verification
admin-channels-email-toggle-insecure-description = ONLY for Greenmail or self-hosted test servers with a self-signed cert. Leave off in production.
admin-channels-email-test = Test connection
admin-channels-email-testing = Testing...
admin-channels-email-test-connected = Connected
admin-channels-email-test-failed = Failed
admin-channels-email-test-unknown-error = Unknown error
admin-channels-email-delete = Delete
admin-channels-email-deleting = Deleting...
admin-channels-email-save = Save changes
admin-channels-email-saving = Saving...
admin-channels-email-create = Create channel
admin-channels-email-creating = Creating...
admin-channels-email-clear-credential-title = Remove stored password?
admin-channels-email-clear-credential-message = The worker will stop authenticating until a new one is saved.
admin-channels-email-clear-credential-confirm = Remove
admin-channels-email-delete-title = Delete this email channel?
admin-channels-email-delete-message = Tickets already opened from it stay intact, but no new messages will be ingested. This cannot be undone.
admin-channels-email-delete-confirm = Delete channel
admin-channels-email-relative-seconds = { $count }s ago
admin-channels-email-relative-minutes = { $count }m ago
admin-channels-email-relative-hours = { $count }h ago
admin-channels-email-relative-days = { $count }d ago
admin-channels-email-error-load = Failed to load email channel
admin-channels-email-success-update = Channel updated
admin-channels-email-success-create = Channel created
admin-channels-email-success-password-removed = Password removed
admin-channels-email-success-delete = Channel deleted

# Admin: Microsoft Graph (data import)
admin-msgraph-back = Back to Data Import
admin-msgraph-title = Microsoft Graph
admin-msgraph-subtitle = Manage data synchronization from Microsoft 365 services
admin-msgraph-sync-action = Sync Data
admin-msgraph-syncing = Syncing...
admin-msgraph-api-name = Microsoft Graph API
admin-msgraph-status-connected = Connected
admin-msgraph-status-disconnected = Not Connected
admin-msgraph-status-connecting = Connecting...
admin-msgraph-status-error = Error
admin-msgraph-config-valid = Configured
admin-msgraph-config-invalid = Not Configured
admin-msgraph-field-client-id = Client ID
admin-msgraph-field-tenant-id = Tenant ID
admin-msgraph-field-secret = Secret
admin-msgraph-field-not-set = Not set
admin-msgraph-secret-configured = Configured
admin-msgraph-secret-not-set = Not Set
admin-msgraph-last-synced = Last synchronized:
admin-msgraph-missing-config = Missing required configuration:
admin-msgraph-env-label = Env:
admin-msgraph-progress-title = Synchronizing
admin-msgraph-progress-step = Step { $current } of { $total }
admin-msgraph-progress-status-running = running
admin-msgraph-progress-status-starting = starting
admin-msgraph-progress-status-completed = completed
admin-msgraph-progress-status-completed-with-errors = Completed with errors
admin-msgraph-progress-status-cancelling = cancelling
admin-msgraph-progress-status-cancelled = cancelled
admin-msgraph-progress-status-error = error
admin-msgraph-cancel = Cancel
admin-msgraph-monitor = Monitor
admin-msgraph-delta-badge = Delta
admin-msgraph-last-sync-title = Last Synchronization
admin-msgraph-last-sync-status-completed = Completed
admin-msgraph-last-sync-status-completed-with-errors = Completed with Errors
admin-msgraph-last-sync-status-error = Error
admin-msgraph-last-sync-status-cancelled = Cancelled
admin-msgraph-last-sync-type = Type
admin-msgraph-last-sync-type-delta = Delta
admin-msgraph-last-sync-type-full = Full
admin-msgraph-last-sync-started = Started
admin-msgraph-last-sync-duration = Duration
admin-msgraph-last-sync-items-processed = Items processed
admin-msgraph-last-sync-cancelled-value = Cancelled
admin-msgraph-last-sync-failed-value = Failed
admin-msgraph-modal-title = Sync Data from Microsoft Graph
admin-msgraph-modal-description = Select the data entities you want to import from Microsoft Graph:
admin-msgraph-entity-users-name = Users
admin-msgraph-entity-users-description = Import user accounts and profiles from Microsoft Entra ID
admin-msgraph-entity-devices-name = Devices
admin-msgraph-entity-devices-description = Import managed devices from Microsoft Intune with user assignments
admin-msgraph-entity-groups-name = Groups
admin-msgraph-entity-groups-description = Import security and distribution groups from Microsoft Entra ID
admin-msgraph-modal-info = Synchronization will import the latest data from Microsoft services. This may take several minutes depending on the amount of data.
admin-msgraph-results-title = Sync Results
admin-msgraph-results-items = { $processed } / { $total } items
admin-msgraph-results-percent = ({ $percent }%)
admin-msgraph-results-more-errors = ... and { $count } more errors
admin-msgraph-results-total-processed = Total processed:
admin-msgraph-results-total-processed-value = { $count } items
admin-msgraph-results-total-errors = Total errors:
admin-msgraph-full-sync = Full sync
admin-msgraph-start-sync = Start Sync
admin-msgraph-starting = Starting...
admin-msgraph-sync-type-users = User Accounts
admin-msgraph-sync-type-profile-photos = Profile Photos
admin-msgraph-sync-type-devices = Managed Devices
admin-msgraph-sync-type-groups = Security Groups
admin-msgraph-time-just-now = Just now
admin-msgraph-time-minutes = { $count }m ago
admin-msgraph-time-hours = { $count }h ago
admin-msgraph-time-days = { $count }d ago
admin-msgraph-duration-seconds = { $seconds }s
admin-msgraph-duration-minutes = { $minutes }m { $seconds }s
admin-msgraph-duration-hours = { $hours }h { $minutes }m
admin-msgraph-error-validate-config = Failed to validate configuration
admin-msgraph-error-fetch-status = Failed to fetch connection status
admin-msgraph-error-start-sync = Failed to start sync
admin-msgraph-error-cancel-sync = Failed to cancel sync
admin-msgraph-success-sync-started = Sync started successfully
admin-msgraph-success-cancel-requested = Sync cancellation requested

# Admin: Plugin registry (browse and install)
admin-plugins-registry-back = Installed plugins
admin-plugins-registry-title = Plugin registry
admin-plugins-registry-subtitle-before = Browse and install plugins published to
admin-plugins-registry-subtitle-after = . Signatures are verified against the Nosdesk root key before any bundle executes.
admin-plugins-registry-refresh = Refresh
admin-plugins-registry-refreshing = Refreshing
admin-plugins-registry-loading = Loading registry...
admin-plugins-registry-disabled-title = Registry sync is disabled
admin-plugins-registry-disabled-description-sideload = This instance has NOSDESK_REGISTRY_URL set to empty, so it isn't fetching the published plugin catalog. You can still sideload a signed zip.
admin-plugins-registry-disabled-description-cli = This instance has NOSDESK_REGISTRY_URL set to empty, so it isn't fetching the published plugin catalog. Use the CLI to install local-signed plugins.
admin-plugins-registry-disabled-action = Sideload signed zip
admin-plugins-registry-pending-title = Registry is syncing
admin-plugins-registry-pending-description = The instance is fetching the published plugin catalog. This usually completes within a few seconds of boot.
admin-plugins-registry-failed-title = Registry sync failed
admin-plugins-registry-failed-description = { $reason }. Retry now to fetch again, or wait for the next scheduled attempt.
admin-plugins-registry-retry-now = Retry now
admin-plugins-registry-search-label = Search plugins
admin-plugins-registry-search-placeholder = Search plugins
admin-plugins-registry-filter-aria = Filter registry
admin-plugins-registry-trust-tier = Trust tier
admin-plugins-registry-tier-official = Official
admin-plugins-registry-tier-verified = Verified
admin-plugins-registry-tier-community = Community
admin-plugins-registry-tier-local = Local
admin-plugins-registry-reset-filters = Reset filters
admin-plugins-registry-snapshot-fetched = Snapshot fetched { $relative }
admin-plugins-registry-result-count = { $filtered } of { $total } { $total ->
    [one] plugin
   *[other] plugins
   }
admin-plugins-registry-no-matches = No plugins match those filters.
admin-plugins-registry-installed-badge = Installed
admin-plugins-registry-manage = Manage
admin-plugins-registry-install = Install
admin-plugins-registry-installing = Installing...
admin-plugins-registry-sr-plugin-name = Plugin name
admin-plugins-registry-sr-publisher = Publisher
admin-plugins-registry-sr-homepage = Homepage
admin-plugins-registry-by-publisher = by { $publisher }
admin-plugins-registry-homepage-link = Homepage
admin-plugins-registry-publisher-nosdesk = Nosdesk
admin-plugins-registry-publisher-unknown = Unknown publisher
admin-plugins-registry-modal-title = Install { $name }?
admin-plugins-registry-community-warning-strong = Community plugin.
admin-plugins-registry-community-warning-body = Nosdesk does not vouch for the safety of community plugins beyond verifying the publisher's signature. Review the source before trusting it with your data.
admin-plugins-registry-field-publisher = Publisher
admin-plugins-registry-field-fingerprint = Fingerprint
admin-plugins-registry-field-version = Version
admin-plugins-registry-type-to-confirm-before = Type
admin-plugins-registry-type-to-confirm-after = to confirm
admin-plugins-registry-cancel = Cancel
admin-plugins-registry-error-load = Failed to load the registry.
admin-plugins-registry-error-refresh = Failed to retry the registry sync.
admin-plugins-registry-error-confirm-name = Type the plugin name exactly to confirm installation.
admin-plugins-registry-error-install = Install failed.
admin-plugins-registry-success-installed = Installed { $name } v{ $version }
admin-plugins-registry-relative-just-now = just now
admin-plugins-registry-relative-minutes = { $count } min ago
admin-plugins-registry-relative-hours = { $count } hr ago
admin-plugins-registry-relative-days = { $count ->
    [one] { $count } day ago
   *[other] { $count } days ago
   }

# Admin: Webhooks (manage outbound event delivery)
admin-webhooks-title = Webhooks
admin-webhooks-subtitle = Manage webhooks for external integrations
admin-webhooks-create = Create Webhook
admin-webhooks-create-short = Create
admin-webhooks-loading = Loading webhooks...
admin-webhooks-section-active = Active Webhooks
admin-webhooks-section-disabled = Disabled Webhooks
admin-webhooks-status-active = Active
admin-webhooks-status-warning = Warning
admin-webhooks-status-failing = Failing
admin-webhooks-status-disabled = Disabled
admin-webhooks-failure-count = { $count ->
    [one] { $count } failure
   *[other] { $count } failures
   }
admin-webhooks-meta-secret = Secret:
admin-webhooks-meta-events = { $count ->
    [one] { $count } event
   *[other] { $count } events
   }
admin-webhooks-meta-last-triggered = Last triggered: { $when }
admin-webhooks-meta-never = Never
admin-webhooks-action-send-test = Send test event
admin-webhooks-action-view-deliveries = View deliveries
admin-webhooks-action-edit = Edit webhook
admin-webhooks-action-delete = Delete webhook
admin-webhooks-modal-create-title = Create Webhook
admin-webhooks-modal-edit-title = Edit Webhook
admin-webhooks-modal-secret-title = Webhook Created
admin-webhooks-modal-regenerate-title = Regenerate Secret
admin-webhooks-modal-delete-title = Delete Webhook
admin-webhooks-modal-deliveries-title = Delivery History - { $name }
admin-webhooks-form-name-label = Name
admin-webhooks-form-name-placeholder = e.g., Slack Notifications
admin-webhooks-form-url-label = Payload URL
admin-webhooks-form-url-placeholder = https://example.com/webhook
admin-webhooks-form-url-hint = POST requests will be sent to this URL
admin-webhooks-form-events-label = Events
admin-webhooks-form-events-hint = Select which events trigger this webhook
admin-webhooks-form-events-count = { $selected }/{ $total }
admin-webhooks-form-headers-label = Custom Headers
admin-webhooks-form-headers-add = + Add header
admin-webhooks-form-headers-name-placeholder = Header name
admin-webhooks-form-headers-value-placeholder = Value
admin-webhooks-form-headers-empty = No custom headers
admin-webhooks-form-enabled-label = Enabled
admin-webhooks-form-enabled-description = Webhook will receive events when enabled
admin-webhooks-form-secret-label = Secret
admin-webhooks-form-secret-regenerate = Regenerate
admin-webhooks-form-cancel = Cancel
admin-webhooks-form-create = Create Webhook
admin-webhooks-form-creating = Creating...
admin-webhooks-form-save = Save Changes
admin-webhooks-form-saving = Saving...
admin-webhooks-secret-warning = Copy this secret now, it won't be shown again!
admin-webhooks-secret-helper-before = Use this secret to verify webhook signatures via the
admin-webhooks-secret-helper-after = header
admin-webhooks-secret-copy = Copy to clipboard
admin-webhooks-secret-copied = Copied!
admin-webhooks-secret-done = Done
admin-webhooks-regenerate-question = Are you sure you want to regenerate the secret for { $name }?
admin-webhooks-regenerate-warning = The current secret will be invalidated. You'll need to update your integration with the new secret.
admin-webhooks-regenerate-confirm = Regenerate
admin-webhooks-regenerate-running = Regenerating...
admin-webhooks-delete-question = Are you sure you want to delete the webhook { $name }?
admin-webhooks-delete-warning = This action cannot be undone. All delivery history will be lost.
admin-webhooks-delete-confirm = Delete Webhook
admin-webhooks-delete-running = Deleting...
admin-webhooks-deliveries-loading = Loading deliveries...
admin-webhooks-deliveries-empty-title = No deliveries yet
admin-webhooks-deliveries-empty-description = Deliveries will appear here once events are triggered
admin-webhooks-deliveries-status-error = Error
admin-webhooks-deliveries-status-pending = Pending
admin-webhooks-deliveries-attempt = Attempt { $number }
admin-webhooks-deliveries-duration = { $ms }ms
admin-webhooks-deliveries-close = Close
admin-webhooks-error-name-required = Webhook name is required
admin-webhooks-error-url-required = URL is required
admin-webhooks-error-event-required = At least one event must be selected
admin-webhooks-error-load = Failed to load webhooks
admin-webhooks-error-create = Failed to create webhook
admin-webhooks-error-update = Failed to update webhook
admin-webhooks-error-delete = Failed to delete webhook
admin-webhooks-error-test = Failed to send test event
admin-webhooks-error-regenerate = Failed to regenerate secret
admin-webhooks-success-update = Webhook updated successfully
admin-webhooks-success-delete = Webhook deleted successfully
admin-webhooks-success-test = Test event sent to webhook
admin-webhooks-success-regenerate = Secret regenerated, check webhook deliveries for the new signature
admin-webhooks-category-tickets = Tickets
admin-webhooks-category-comments = Comments
admin-webhooks-category-attachments = Attachments
admin-webhooks-category-devices = Devices
admin-webhooks-category-projects = Projects
admin-webhooks-category-documentation = Documentation
admin-webhooks-category-users = Users
admin-webhooks-event-ticket-created = Ticket Created
admin-webhooks-event-ticket-updated = Ticket Updated
admin-webhooks-event-ticket-deleted = Ticket Deleted
admin-webhooks-event-ticket-linked = Ticket Linked
admin-webhooks-event-ticket-unlinked = Ticket Unlinked
admin-webhooks-event-comment-added = Comment Added
admin-webhooks-event-comment-deleted = Comment Deleted
admin-webhooks-event-attachment-added = Attachment Added
admin-webhooks-event-attachment-deleted = Attachment Deleted
admin-webhooks-event-device-linked = Device Linked
admin-webhooks-event-device-unlinked = Device Unlinked
admin-webhooks-event-device-updated = Device Updated
admin-webhooks-event-project-assigned = Project Assigned
admin-webhooks-event-project-unassigned = Project Unassigned
admin-webhooks-event-documentation-updated = Documentation Updated
admin-webhooks-event-user-created = User Created
admin-webhooks-event-user-updated = User Updated
admin-webhooks-event-user-deleted = User Deleted

# Users list (UsersListView): people directory with role filter,
# bulk role change, and bulk delete.
user-mgmt-search-placeholder = Search users...
user-mgmt-item-label = user
user-mgmt-filter-all-roles = All Roles
user-mgmt-role-admin = Admin
user-mgmt-role-technician = Technician
user-mgmt-role-user = User
user-mgmt-column-user = User
user-mgmt-column-role = Role
user-mgmt-column-tickets = Tickets
user-mgmt-column-devices = Devices
user-mgmt-column-joined = Joined
user-mgmt-invite-action = Invite User
user-mgmt-mobile-tickets = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
   }
user-mgmt-mobile-devices = { $count ->
    [one] { $count } device
   *[other] { $count } devices
   }
user-mgmt-bulk-role = Role
user-mgmt-bulk-delete = Delete
user-mgmt-bulk-delete-count = Delete { $count }
user-mgmt-bulk-delete-title = { $count ->
    [one] Delete user?
   *[other] Delete { $count } users?
}
user-mgmt-bulk-delete-message = { $count ->
    [one] This will permanently delete one user. This action cannot be undone.
   *[other] This will permanently delete { $count } users. This action cannot be undone.
}
user-mgmt-bulk-action-error = Failed to perform bulk action. Please try again.
user-mgmt-role-modal-title = Set Role
user-mgmt-role-modal-body = { $count ->
    [one] Update role for { $count } user
   *[other] Update role for { $count } users
   }

# User profile (UserProfileView): single user detail + create-user
# form, devices/groups panels, assigned & requested ticket panes.
user-profile-document-title = { $name }'s Profile | Nosdesk
user-profile-back-to-users = Back to Users
user-profile-action-profile-settings = Profile Settings
user-profile-action-user-settings = User Settings
user-profile-create-title = Create New User
user-profile-create-subtitle = Add a new user to your organization
user-profile-section-basic-info = Basic Information
user-profile-field-name = Full Name
user-profile-field-name-placeholder = Enter full name
user-profile-field-email = Email Address
user-profile-field-email-placeholder = user@example.com
user-profile-field-role = Role
user-profile-field-role-placeholder = Select a role
user-profile-role-user = User
user-profile-role-technician = Technician
user-profile-role-admin = Admin
user-profile-field-pronouns = Pronouns
user-profile-field-pronouns-placeholder = e.g., he/him, she/her, they/them
user-profile-section-account-setup = Account Setup
user-profile-smtp-warning-title = Email not configured
user-profile-smtp-warning-body = You must set a password manually since email invitations are unavailable.
user-profile-setup-method = Setup Method
user-profile-setup-invite-title = Send invitation email
user-profile-setup-invite-body = User will receive an email with a secure link to set their own password
user-profile-setup-password-title = Set password manually
user-profile-setup-password-body = Create a password for the user now and share it with them securely
user-profile-field-password = Password
user-profile-field-password-placeholder = Minimum 8 characters
user-profile-field-confirm-password = Confirm Password
user-profile-field-confirm-password-placeholder = Re-enter password
user-profile-passwords-match = Passwords match
user-profile-passwords-no-match = Passwords do not match
user-profile-required-note = Required fields
user-profile-action-cancel = Cancel
user-profile-action-create = Create User
user-profile-action-creating = Creating...
user-profile-devices-title = Devices
user-profile-devices-empty = No devices
user-profile-device-manufacturer-unknown = Unknown
user-profile-device-last-updated = Last updated { $when }
user-profile-groups-title = Groups
user-profile-not-found = User not found
user-profile-error-no-create-permission = You do not have permission to create users
user-profile-error-missing-id = User ID is missing
user-profile-error-password-too-short = Password must be at least 8 characters long
user-profile-error-passwords-mismatch = Passwords do not match
user-profile-error-created-no-uuid = User created but navigation failed. Please go to Users list.
user-profile-error-save-generic = Failed to save user. Please try again.
user-profile-error-load = Failed to load user profile
user-profile-relative-just-now = just now
user-profile-relative-minutes-ago = { $count ->
    [one] { $count } minute ago
   *[other] { $count } minutes ago
   }
user-profile-relative-hours-ago = { $count ->
    [one] { $count } hour ago
   *[other] { $count } hours ago
   }
user-profile-relative-days-ago = { $count ->
    [one] { $count } day ago
   *[other] { $count } days ago
   }

# Groups management (GroupsManagementView): list, search/sort,
# create modal, delete confirm, and member/device/group count chips.
groups-mgmt-title = Groups
groups-mgmt-subtitle = Manage user groups and memberships
groups-mgmt-action-new = New Group
groups-mgmt-action-new-short = New
groups-mgmt-loading = Loading groups...
groups-mgmt-search-placeholder = Search groups...
groups-mgmt-sort-name = Name
groups-mgmt-sort-members = Members
groups-mgmt-sort-devices = Devices
groups-mgmt-sort-created = Date Added
groups-mgmt-sort-ascending = Ascending
groups-mgmt-sort-descending = Descending
groups-mgmt-chip-members = { $count ->
    [one] { $count } member
   *[other] { $count } members
   }
groups-mgmt-chip-devices = { $count ->
    [one] { $count } device
   *[other] { $count } devices
   }
groups-mgmt-chip-groups = { $count ->
    [one] { $count } group
   *[other] { $count } groups
   }
groups-mgmt-action-open-full-page = Open full page
groups-mgmt-action-delete = Delete group
groups-mgmt-no-results = No groups matching "{ $query }"
groups-mgmt-empty-action = Create Group
groups-mgmt-modal-create-title = Create Group
groups-mgmt-field-name = Name
groups-mgmt-field-name-placeholder = Enter group name
groups-mgmt-field-description = Description
groups-mgmt-field-description-placeholder = Optional description
groups-mgmt-field-color = Color
groups-mgmt-action-cancel = Cancel
groups-mgmt-action-create = Create Group
groups-mgmt-modal-delete-title = Delete Group
groups-mgmt-delete-confirm-body = Are you sure you want to delete the group <strong class="text-primary">{ $name }</strong>? This will remove all member associations but will not delete the users.
groups-mgmt-action-delete-confirm = Delete Group
groups-mgmt-error-name-required = Group name is required
groups-mgmt-error-load = Failed to load groups
groups-mgmt-error-create = Failed to create group
groups-mgmt-error-delete = Failed to delete group
groups-mgmt-success-created = Group created successfully
groups-mgmt-success-deleted = Group deleted successfully

# Group detail (GroupDetailView): per-group page showing
# sync status, members, devices, and creation metadata.
group-detail-error-invalid-id = Invalid group ID
group-detail-error-load = Failed to load group details
group-detail-sync-source-microsoft = Microsoft Entra ID
group-detail-type-security = Security
group-detail-type-mail-enabled = Mail-enabled
group-detail-type-standard = Standard
group-detail-synced-from = Synced from { $source }
group-detail-action-configure = Configure
group-detail-section-information = Group Information
group-detail-field-type = Type
group-detail-field-sync-source = Sync Source
group-detail-field-last-synced = Last Synced
group-detail-field-created = Created
group-detail-field-updated = Updated
group-detail-section-members = Members
group-detail-section-devices = Devices
group-detail-no-members = No members
group-detail-no-devices = No devices
group-detail-unknown-device = Unknown device
group-detail-not-found = Group not found

# Devices list (DevicesListView): paginated table with warranty
# filter, sortable columns, bulk delete, and mobile row layout.
devices-list-search-placeholder = Search devices...
devices-list-item-label = device
devices-list-filter-warranty-active = Active
devices-list-filter-warranty-warning = Warning
devices-list-filter-warranty-expired = Expired
devices-list-filter-warranty-unknown = Unknown
devices-list-filter-warranty-all = All Warranties
devices-list-column-device = Device
devices-list-column-serial = Serial
devices-list-column-hostname = Hostname
devices-list-column-model = Model
devices-list-column-user = User
devices-list-column-warranty = Warranty
devices-list-add-action = Add Device
devices-list-unassigned = Unassigned
devices-list-warranty-unknown = Unknown
devices-list-bulk-delete = Delete
devices-list-bulk-delete-count = Delete { $count }
devices-list-bulk-delete-title = { $count ->
    [one] Delete device?
   *[other] Delete { $count } devices?
}
devices-list-bulk-delete-message = { $count ->
    [one] This will permanently delete one device. This action cannot be undone.
   *[other] This will permanently delete { $count } devices. This action cannot be undone.
}
devices-list-bulk-action-error = Failed to delete devices. Please try again.

# Device detail (DeviceView): per-device page covering name, hostname,
# hardware identifiers, warranty fields, primary user, Microsoft Intune
# integration, and the unmanage / create flows.
device-detail-back-to-ticket = Back to Ticket #{ $id }
device-detail-back-to-devices = Go back
device-detail-readonly = Read-only
device-detail-delete-item-name = Device
device-detail-error-invalid-id = Invalid device ID
device-detail-error-load = Failed to load device details
device-detail-error-create = Failed to create device. Please try again.
device-detail-error-delete = Failed to delete device. Please try again.
device-detail-error-unmanage = Failed to unmanage device. Please try again.
device-detail-section-details = Device Details
device-detail-field-name = Name
device-detail-field-name-placeholder-create = Enter device name
device-detail-field-name-placeholder-edit = Enter name...
device-detail-field-hostname = Hostname
device-detail-field-hostname-placeholder-create = Enter hostname
device-detail-field-hostname-placeholder-edit = Enter hostname...
device-detail-field-serial = Serial Number
device-detail-field-serial-placeholder-create = Enter serial number
device-detail-field-serial-placeholder-edit = Enter serial number...
device-detail-field-manufacturer = Manufacturer
device-detail-field-manufacturer-placeholder-create = e.g., Dell, HP, Apple
device-detail-field-manufacturer-placeholder-edit = Enter manufacturer...
device-detail-field-model = Model
device-detail-field-model-placeholder-create = Enter device model
device-detail-field-model-placeholder-edit = Enter model...
device-detail-field-warranty-status = Warranty Status
device-detail-field-warranty-start = Warranty Start
device-detail-field-warranty-end = Warranty End
device-detail-field-purchase-date = Purchase Date
device-detail-field-asset-tag = Asset Tag
device-detail-field-asset-tag-placeholder-create = Enter asset tag
device-detail-field-asset-tag-placeholder-edit = Enter asset tag...
device-detail-warranty-active = Active
device-detail-warranty-warning = Warning
device-detail-warranty-expired = Expired
device-detail-warranty-unknown = Unknown
device-detail-section-primary-user = Primary User
device-detail-no-user-assigned = No user assigned to this device
device-detail-action-assign-user = Assign User
device-detail-action-change-user = Change User
device-detail-section-device-information = Device Information
device-detail-field-device-id = Device ID
device-detail-field-created = Created
device-detail-field-last-updated = Last Updated
device-detail-manually-managed = Manually Managed
device-detail-manually-managed-description = This device was created and is managed manually in Nosdesk
device-detail-section-microsoft-integration = Microsoft Integration
device-detail-field-last-intune-check-in = Last Intune Check-in
device-detail-action-view-in-intune = View in Intune
device-detail-action-view-in-entra = View in Entra
device-detail-action-unmanage = Unmanage from Intune/Entra
device-detail-action-unmanage-processing = Processing...
device-detail-action-unmanage-title = Remove from Microsoft Intune/Entra management
device-detail-unmanage-conversion-note = This will convert the device to manual management
device-detail-tech-details-show = Show Technical Details
device-detail-tech-details-hide = Hide Technical Details
device-detail-field-intune-id = Intune ID
device-detail-field-entra-id = Entra ID
device-detail-not-managed-by-intune = This device is not managed by Microsoft Intune
device-detail-action-cancel = Cancel
device-detail-action-create = Create Device
device-detail-action-create-processing = Creating...
device-detail-not-found = Device not found
device-detail-unmanage-modal-title = Unmanage Device
device-detail-unmanage-heading = Unmanage from Microsoft
device-detail-unmanage-confirm-body = Are you sure you want to unmanage <strong class="text-primary">{ $name }</strong> from Microsoft Intune/Entra?
device-detail-unmanage-confirm-note = This will convert the device to manual management. You'll be able to edit all fields, but the device will no longer sync with Microsoft.
device-detail-unmanage-action-confirm = Unmanage

# Projects list (ProjectsView): workspace-wide grid of projects
# rendered from the sync engine pool, with status pills and a
# short description per card.
projects-list-heading = Projects
projects-list-subheading = Sync-engine preview (projects_v2 flag).
projects-list-no-description = No description

# Project detail (ProjectDetailView): per-project kanban board
# with a header, status pill, ticket count, and a Group-by
# control on the kanban toolbar.
project-detail-loading-name = Loading…
project-detail-ticket-count = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
   }
project-detail-group-by-label = Group by
project-detail-group-by-status = Status only
project-detail-group-by-assignee = Status x Assignee
project-detail-group-by-priority = Status x Priority
project-detail-loading = Loading project…

# Project Gantt (ProjectGanttView): per-project Gantt timeline
# with a header summary of ticket and dependency-link counts.
project-gantt-fallback-name = Project
project-gantt-summary = { $tickets ->
    [one] { $tickets } ticket
   *[other] { $tickets } tickets
   } · { $links ->
    [one] { $links } link
   *[other] { $links } links
   }

# Project cycles (ProjectCyclesView): full-page cycles surface
# with active-cycle burndown, create form, and a list of every
# cycle for the project (planned / active / completed).
project-cycles-fallback-name = Project
project-cycles-count = { $count ->
    [one] { $count } cycle
   *[other] { $count } cycles
   }
project-cycles-new-button = New cycle
project-cycles-cancel-button = Cancel
project-cycles-date-missing = —
project-cycles-confirm-complete = Complete this cycle? The snapshot freezes once you do.
project-cycles-confirm-archive = Archive this cycle?
project-cycles-create-title = New cycle
project-cycles-field-name = Name
project-cycles-field-start = Start
project-cycles-field-end = End
project-cycles-name-placeholder = e.g. Sprint 14
project-cycles-create-submit = Create
project-cycles-all-title = All cycles
project-cycles-empty-prefix = No cycles yet. Click
project-cycles-empty-cta = New cycle
project-cycles-empty-suffix = to start an iteration.
project-cycles-state-planned = planned
project-cycles-state-active = active
project-cycles-state-completed = completed
project-cycles-action-promote = Promote
project-cycles-action-complete = Complete
project-cycles-action-archive = Archive

# Workspace cycles (WorkspaceCyclesView): cross-project overview
# of in-flight iterations, grouped by project, with a toggle to
# pull completed cycles back into view.
workspace-cycles-heading = Cycles
workspace-cycles-subheading = In-flight iterations across every project
workspace-cycles-show-completed = Show completed
workspace-cycles-loading = Loading cycles…
workspace-cycles-error-fallback = Failed to load cycles
workspace-cycles-empty-title = No cycles yet.
workspace-cycles-empty-hint = Open a project and start one from the Cycles drawer.
workspace-cycles-group-count = { $count ->
    [one] { $count } cycle
   *[other] { $count } cycles
   }
workspace-cycles-project-fallback = Project #{ $id }
workspace-cycles-date-missing = —
workspace-cycles-state-planned = planned
workspace-cycles-state-completed = completed

# Cycle detail (CycleDetailView): Scrum board scoped to one
# cycle, with a burndown pinned above the kanban toolbar.
cycle-detail-back = ‹ Cycles
cycle-detail-loading-name = Loading…
cycle-detail-loading = Loading cycle…
cycle-detail-error-fallback = Failed to load cycle
cycle-detail-summary = { $state } · { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
   }
cycle-detail-group-by-label = Group by
cycle-detail-group-by-status = Status only
cycle-detail-group-by-assignee = Status x Assignee
cycle-detail-group-by-priority = Status x Priority
cycle-detail-state-planned = Planned
cycle-detail-state-active = Active
cycle-detail-state-completed = Completed

# Documentation index (DocumentationIndexView): hub page listing
# recently updated, starred, collections, and status chips.
docs-index-title = Documentation
docs-index-new-page = New page
docs-index-recently-updated = Recently updated
docs-index-recently-updated-count = Last { $count }
docs-index-no-recent-activity = No recent activity.
docs-index-starred = Starred
docs-index-starred-hint = Star a page from its row menu for quick access.
docs-index-browse-all = Browse all pages
docs-index-chip-drafts = { $count ->
    [one] { $count } draft
   *[other] { $count } drafts
   }
docs-index-chip-archived = { $count } archived
docs-index-chip-trash = { $count } in trash

# Documentation drafts (DocumentationDraftsView): pages not yet
# assigned to a collection.
docs-drafts-title = Drafts
docs-drafts-heading = Drafts
docs-drafts-description = Pages not yet assigned to a collection
docs-drafts-back = Back to Documentation
docs-drafts-count = { $count ->
    [one] { $count } page
   *[other] { $count } pages
   }

# Documentation archived (DocumentationArchivedView): list of
# pages that have been archived, with a restore action.
docs-archived-title = Archived
docs-archived-heading = Archived
docs-archived-description = Pages that have been archived
docs-archived-back = Back to Documentation
docs-archived-count = { $count ->
    [one] { $count } page
   *[other] { $count } pages
   }
docs-archived-loading = Loading archived pages
docs-archived-archived-at = Archived { $date }
docs-archived-restore = Restore

# Documentation trash (DocumentationTrashView): deleted pages,
# with restore and permanent-delete actions.
docs-trash-title = Trash
docs-trash-heading = Trash
docs-trash-description = Deleted pages can be restored or permanently removed
docs-trash-back = Back to Documentation
docs-trash-count = { $count ->
    [one] { $count } page
   *[other] { $count } pages
   }
docs-trash-loading = Loading trashed pages
docs-trash-deleted-at = Deleted { $date }
docs-trash-restore = Restore
docs-trash-delete-forever = Delete forever
docs-trash-confirm-delete = Confirm delete?

# Documentation gaps (DocumentationGapsView): queue of open
# knowledge gaps with a list pane and detail pane.
docs-gaps-title = Knowledge Gaps
docs-gaps-heading = Knowledge Gaps
docs-gaps-back-docs = Docs
docs-gaps-back-list = Knowledge Gaps
docs-gaps-refresh = Refresh signals
docs-gaps-refreshing = Refreshing
docs-gaps-detect-no-results = No new clusters found
docs-gaps-detect-created = { $count } new
docs-gaps-detect-updated = { $count } updated
docs-gaps-loading = Loading
docs-gaps-empty = No open knowledge gaps. Flag a ticket from its sidebar to add one.
docs-gaps-impact-searches = searches
docs-gaps-impact-recent-tickets = recent tickets
docs-gaps-impact-tickets = tickets
docs-gaps-impact-tooltip = { $count } { $label } representing demand for this doc
docs-gaps-signal-count = { $count ->
    [one] { $count } signal
   *[other] { $count } signals
   }
docs-gaps-select-prompt = Select a gap from the list to see its evidence.
docs-gaps-status-label = Status:
docs-gaps-last-evidence = Last evidence: { $time }
docs-gaps-dismiss = Dismiss
docs-gaps-evidence-heading = Evidence
docs-gaps-evidence-empty = No evidence rows.
docs-gaps-signal-manual-flag = Manual flag
docs-gaps-signal-ticket-cluster = Ticket cluster
docs-gaps-signal-failed-search = Failed search
docs-gaps-signal-stale-doc = Stale doc
docs-gaps-signal-ai-suggested = AI suggestion
docs-gaps-cluster-fallback = Cluster
docs-gaps-cluster-via = via { $channel }
docs-gaps-cluster-more = { $count ->
    [one] and { $count } more
   *[other] and { $count } more
   }
docs-gaps-stale-untitled = Untitled doc
docs-gaps-stale-verified = Verified { $time }
docs-gaps-stale-verified-no-time = Verified
docs-gaps-stale-days-past-due = { $count ->
    [one] { $count } day past due
   *[other] { $count } days past due
   }
docs-gaps-stale-recent-tickets = { $count ->
    [one] ticket closed recently still cites this doc:
   *[other] tickets closed recently still cite this doc:
   }
docs-gaps-stale-plus-more = + { $count } more
docs-gaps-stale-auto-dismiss = Re-verifying the doc will auto-dismiss this gap.
docs-gaps-failed-search-count = { $count ->
    [one] { $count } search with no results
   *[other] { $count } searches with no results
   }
docs-gaps-failed-search-range = first { $first }, last { $last }
docs-gaps-flagged-by = Flagged by { $name }
docs-gaps-resolve-heading = Resolve this gap
docs-gaps-resolve-body = Open one of the tickets above and use { $action } from its sidebar. The new doc will auto-link as 'resolves' on every flagged ticket.
docs-gaps-resolve-action = Save as doc

# Document view (DocumentView): full-page editor for a single doc
# page (or a ticket note). Covers the header toolbar, metadata
# strip, save indicators, verification chips, panels, and toasts.
doc-detail-back-to-ticket = Back to Ticket
doc-detail-back-to-documentation = Back to Documentation
doc-detail-saving = Saving
doc-detail-publish = Publish
doc-detail-star = Star page
doc-detail-unstar = Unstar page
doc-detail-copy-link = Copy link
doc-detail-copied = Copied
doc-detail-untitled = Untitled
doc-detail-status-draft = Draft
doc-detail-status-archived = Archived
doc-detail-needs-verification = Needs verification
doc-detail-needs-verification-title = Verify this page
doc-detail-verification-stale = Verification stale
doc-detail-verification-stale-title = Re-verify this page
doc-detail-sse-live = Live updates active
doc-detail-sse-connecting = Connecting
doc-detail-sse-disconnected = Disconnected
doc-detail-history = History
doc-detail-history-title = Revision history
doc-detail-editor-placeholder = Enter documentation content here
doc-detail-not-found-title = Document not found
doc-detail-not-found-body = The document you're looking for doesn't exist or has been moved.
doc-detail-not-found-link = Go to Documentation Home
doc-detail-toast-deleting = Deleting document
doc-detail-toast-deleted = Document deleted successfully
doc-detail-toast-delete-error = Error deleting document
doc-detail-duplicate-suffix = { $title } (copy)
doc-detail-ticket-note-title = Notes for Ticket #{ $id }
doc-detail-ticket-note-description = Documentation for ticket { $title }
doc-detail-ticket-note-author-system = System

# Asset planner (AssetPlannerView): rollout-planning kanban for
# devices, grouped by OS family, warranty bucket, or compliance
# state. Covers the header, sidebar filters, group columns, and
# device card chips.
asset-planner-title = Assets
asset-planner-subtitle = Plan rollouts by OS, warranty, or compliance state.
asset-planner-search-placeholder = Search by name, hostname, model…
asset-planner-group-by = Group by
asset-planner-axis-os = OS family
asset-planner-axis-warranty = Warranty
asset-planner-axis-compliance = Compliance
asset-planner-loading = Loading assets…
asset-planner-load-error = Failed to load assets
asset-planner-filters-heading = Filters
asset-planner-filters-clear = Clear ({ $count })
asset-planner-section-os = OS
asset-planner-section-warranty = Warranty
asset-planner-section-compliance = Compliance
asset-planner-count = { $visible } of { $total ->
    [one] { $total } device
   *[other] { $total } devices
   }
asset-planner-empty = No devices match the current filters.
asset-planner-warranty-ends = Warranty ends { $date }
asset-planner-no-warranty-data = No warranty data
asset-planner-warranty-unknown-short = n/a
asset-planner-card-host = Host
asset-planner-card-os = OS
asset-planner-card-model = Model
asset-planner-card-tag = Tag
asset-planner-card-compliance = Compliance
asset-planner-os-windows = Windows
asset-planner-os-macos = macOS
asset-planner-os-linux = Linux
asset-planner-os-ios = iOS
asset-planner-os-android = Android
asset-planner-os-other = Other
asset-planner-warranty-expired = Expired
asset-planner-warranty-expiring-30d = Expiring in 30 days
asset-planner-warranty-expiring-90d = Expiring in 90 days
asset-planner-warranty-active = Active
asset-planner-warranty-unknown = Unknown
asset-planner-compliance-unknown = Unknown

# Collection view (CollectionView): documentation collection
# detail page with editable name/icon, an overview editor,
# visibility chips, an expandable list of pages with custom
# permissions, and the collection's page tree.
collection-back-to-documentation = Back to Documentation
collection-not-found-title = Collection Not Found
collection-action-delete = Delete
collection-action-manage-access = Manage Access
collection-action-new-page = New Page
collection-new-page-default-title = New Page
collection-not-found-heading = Collection not found
collection-not-found-description = This collection may have been moved or deleted.
collection-badge-system = System
collection-badge-restricted = Restricted
collection-badge-public = Public
collection-overview-heading = Overview
collection-overview-placeholder = Write an overview for this collection...
collection-overrides-summary = { $count ->
    [one] { $count } page with custom permissions
   *[other] { $count } pages with custom permissions
   }
collection-pages-heading = Pages
collection-page-count = { $count ->
    [one] { $count } page
   *[other] { $count } pages
   }
collection-delete-title = Delete { $name }?
collection-delete-title-fallback = Delete collection?
collection-delete-message = Pages in this collection will not be deleted.
collection-delete-confirm = Delete

# CSV import (CsvImportView): admin page for importing users,
# devices, or tickets from CSV. Covers the page header, status
# messages, action buttons, the import status card, guideline
# panels, the template list, and both modals (file upload and
# template download).
csv-import-back = Back to Data Import
csv-import-title = CSV Import
csv-import-subtitle = Import data from CSV files into your system
csv-import-action-import = Import Data
csv-import-action-templates = Download Templates
csv-import-status-heading = Import Status
csv-import-status-success = Import Completed
csv-import-status-in-progress = Import in Progress
csv-import-status-error = Import Failed
csv-import-last-import = Last import: { $date }
csv-import-results-total = Total Records
csv-import-results-successful = Successful
csv-import-results-failed = Failed
csv-import-guidelines-heading = CSV Import Guidelines
csv-import-requirements-heading = CSV File Requirements
csv-import-requirements-utf8 = Files must be in CSV format with UTF-8 encoding
csv-import-requirements-headers = The first row must contain column headers matching the expected fields
csv-import-requirements-required = Required fields must not be empty
csv-import-requirements-date-format = Date fields should use the format YYYY-MM-DD
csv-import-requirements-max-size = Maximum file size: 10MB
csv-import-notes-heading = Important Notes
csv-import-notes-updates = Existing records will be updated if they share a unique identifier (like email or ID)
csv-import-notes-validation = Data validation is performed before import, records with invalid data will be skipped
csv-import-notes-duration = For large imports, the process may take several minutes to complete
csv-import-notes-templates = Download and use our template files to ensure proper formatting
csv-import-templates-heading = Available Templates
csv-import-templates-intro = Use these templates as a starting point for your CSV imports
csv-import-template-users-name = Users Template
csv-import-template-users-description = Import user accounts with roles and contact information
csv-import-template-devices-name = Devices Template
csv-import-template-devices-description = Import devices with hardware details and ownership information
csv-import-template-tickets-name = Tickets Template
csv-import-template-tickets-description = Import support tickets with details and assignees
csv-import-template-download = Download
csv-import-modal-import-title = Import Data from CSV
csv-import-modal-data-type = Data Type
csv-import-modal-type-users = Users
csv-import-modal-type-devices = Devices
csv-import-modal-type-tickets = Tickets
csv-import-modal-file-label = CSV File
csv-import-modal-upload-link = Upload a file
csv-import-modal-drag-drop = or drag and drop
csv-import-modal-size-hint = CSV files up to 10MB
csv-import-modal-cancel = Cancel
csv-import-modal-start = Start Import
csv-import-modal-starting = Importing...
csv-import-modal-templates-title = CSV Templates
csv-import-modal-templates-intro = Download our CSV templates to ensure your data is formatted correctly for import.
csv-import-modal-fields-count = { $count ->
    [one] { $count } field
   *[other] { $count } fields
   }
csv-import-modal-close = Close
csv-import-error-no-file = Please select a file to import
csv-import-error-failed = Import failed
csv-import-error-generic = Failed to import data
csv-import-success-completed = Import completed successfully
csv-import-toast-template-downloaded = { $type } template downloaded

# Error page (ErrorView)
error-page-default-code = 404
error-page-default-message = Page not found
error-page-description = The page you're looking for doesn't exist or you may not have access to it.
error-page-go-back = Go back
error-page-go-home = Go to Dashboard
error-page-debug-title = Debug Controls (press 'd' to toggle)
error-page-debug-master-toggle = Master Effects Toggle
error-page-debug-global-intensity = Global Intensity
error-page-debug-channel-separation = Channel Separation
error-page-debug-distortion-scale = Distortion Scale
error-page-debug-glitch-frequency = Glitch Frequency
error-page-debug-glitch-intensity = Glitch Intensity
error-page-debug-cursor-influence = Cursor Influence

# PDF viewer (PDFViewerView)
pdf-viewer-default-filename = Document
pdf-viewer-back = Back
pdf-viewer-share = Share
pdf-viewer-share-tooltip = Copy link to clipboard
pdf-viewer-loading = Loading PDF document...
pdf-viewer-error-title = Error Loading PDF
pdf-viewer-error-go-back = Go Back
pdf-viewer-error-no-source = No PDF source provided
pdf-viewer-error-failed = Failed to load PDF
pdf-viewer-error-failed-with-reason = Failed to load PDF: { $reason }
pdf-viewer-error-unknown = Unknown error

# Settings: MFA (MFASettings) - two-factor authentication setup, verify, backup codes, disable
settings-mfa-title = Two-Factor Authentication
settings-mfa-title-success = Setup Complete!
settings-mfa-toggle-label = Enable Two-Factor Authentication
settings-mfa-toggle-description-enabled = Your account is protected with 2FA
settings-mfa-toggle-description-disabled = Secure your account with an authenticator app
settings-mfa-admin-status-enabled = Enabled
settings-mfa-admin-status-disabled = Not enabled
settings-mfa-admin-backup-codes-generated = · Backup codes generated
settings-mfa-admin-disable = Disable
settings-mfa-admin-disabling = Disabling...
settings-mfa-admin-note = MFA setup requires the account owner's authenticator app.
settings-mfa-admin-disable-success = Two-factor authentication has been disabled for this user
settings-mfa-admin-disable-error = Failed to disable MFA
settings-mfa-admin-load-error = Failed to load MFA status for this user
settings-mfa-setup-init-error = Failed to initialize MFA setup
settings-mfa-setup-not-ready = MFA setup not initialized properly
settings-mfa-manual-toggle = Can't scan? Enter the code manually
settings-mfa-manual-instructions = Enter this secret key in your authenticator app:
settings-mfa-copy-button = Copy
settings-mfa-copied-button = Copied!
settings-mfa-copy-tooltip = Copy to clipboard
settings-mfa-copied-tooltip = Copied to clipboard!
settings-mfa-copy-error = Failed to copy to clipboard
settings-mfa-verify-heading = Enter Verification Code
settings-mfa-verify-instructions = Enter the 6-digit code from your authenticator app:
settings-mfa-verify-aria-label = MFA verification code
settings-mfa-verify-button = Verify
settings-mfa-verifying-button = Verifying...
settings-mfa-verify-invalid-length = Please enter a valid 6-digit code
settings-mfa-verify-missing-secret = MFA secret is missing. Please restart the setup process.
settings-mfa-verify-invalid-code = Invalid verification code. Please try again.
settings-mfa-verify-incomplete-login = MFA enabled but login response was incomplete
settings-mfa-qr-alt = MFA QR Code
settings-mfa-verifying-heading = Verifying Code
settings-mfa-verifying-message = Please wait while we verify your authenticator code...
settings-mfa-disable-password-prompt = Please enter your password to disable MFA:
settings-mfa-backup-codes-heading = Backup Codes
settings-mfa-backup-codes-description = Save these backup codes in a secure location. You can use them to access your account if you lose your authenticator device.
settings-mfa-backup-codes-download = Download
settings-mfa-backup-codes-download-tooltip = Download backup codes as text file
settings-mfa-backup-codes-download-success = Backup codes downloaded successfully
settings-mfa-backup-codes-download-error = Failed to download backup codes
settings-mfa-backup-file-title = Nosdesk Backup Codes
settings-mfa-backup-file-warning = IMPORTANT: Save these backup codes in a secure location.
settings-mfa-backup-file-usage = Each code can only be used once to access your account if you lose your authenticator device.
settings-mfa-backup-file-codes-heading = Backup Codes:
settings-mfa-backup-file-generated = Generated on: { $date }
settings-mfa-success-heading = Two-Factor Authentication Enabled!
settings-mfa-success-message = Your account is now protected with 2FA. You'll need to enter a code from your authenticator app when signing in.
settings-mfa-success-cta = Start Using Nosdesk!

# Settings: auth methods (AuthMethodsSettings) - linked sign-in providers and active session management
settings-auth-methods-section-title = Authentication Methods
settings-auth-methods-type-local = Email / Password
settings-auth-methods-type-microsoft = Microsoft
settings-auth-methods-primary-badge = Primary
settings-auth-methods-added-suffix = · Added { $date }
settings-auth-methods-remove = Remove
settings-auth-methods-connect-microsoft = Connect Microsoft Account
settings-auth-methods-connect-microsoft-already = Already connected
settings-auth-methods-connect-microsoft-provider = Azure AD / Entra ID
settings-auth-methods-link-success = { $provider } account linked successfully
settings-auth-methods-link-error = Failed to link { $provider } account
settings-auth-methods-remove-success = Authentication method removed successfully
settings-auth-methods-remove-error = Failed to remove authentication method
settings-auth-methods-sessions-section-title = Active Sessions
settings-auth-methods-sessions-revoke-all = Revoke All Others
settings-auth-methods-sessions-unknown-device = Unknown Device
settings-auth-methods-sessions-unknown-location = Unknown location
settings-auth-methods-sessions-current-badge = Current
settings-auth-methods-sessions-last-active = { $location } • Last active { $date }
settings-auth-methods-sessions-revoke = Revoke
settings-auth-methods-sessions-revoke-success = Session revoked successfully
settings-auth-methods-sessions-revoke-error = Failed to revoke session
settings-auth-methods-sessions-revoke-all-success = All other sessions revoked successfully
settings-auth-methods-sessions-revoke-all-error = Failed to revoke sessions
settings-auth-methods-sessions-load-error = Failed to load active sessions

# Shared common chrome — keys used by primitive components like
# Modal.vue. Kept under the "common-*" namespace so primitives
# don't depend on feature-specific FTL slices.
common-modal-close = Close modal
