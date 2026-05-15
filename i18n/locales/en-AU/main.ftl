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
# en-AU keeps the same body wording as US for now; the divergence
# we already ship (settings copy, time-zone) is the visible
# demo. Translators can tighten the AU register later.
password-reset-title = Password Reset Request
password-reset-greeting = G'day <strong>{ $name }</strong>,
password-reset-intro = We received a request to reset your password for your <strong>{ $app }</strong> account. If you didn't make this request, you can safely ignore this email.
password-reset-action-prompt = To reset your password, click the button below:
password-reset-cta-label = Reset Password
password-reset-notice-expiry = This link will expire in <strong>1 hour</strong>
password-reset-notice-single-use = This link can only be used <strong>once</strong>
password-reset-notice-never-share = Never share this link with anyone
password-reset-notice-account-security = If you didn't request this reset, please secure your account immediately
password-reset-footer = If you have any questions, please contact your system administrator.
password-reset-body-text =
    G'day { $name },

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
# en-AU keeps the G'day greeting for the demo divergence (see
# password-reset-greeting in this file).
invitation-title = Welcome to { $app }!
invitation-greeting = G'day <strong>{ $name }</strong>,
invitation-intro = You've been invited to join <strong>{ $app }</strong> by <strong>{ $by }</strong>.
invitation-action-prompt = To complete your account setup and create your password, click the button below:
invitation-cta-label = Set Up Your Account
invitation-notice-expiry = This invitation link will expire in <strong>7 days</strong>
invitation-notice-create-password = You'll need to create a password during setup
invitation-notice-strong-password = Choose a strong password with at least 8 characters
invitation-notice-unexpected = If you didn't expect this invitation, you can safely ignore this email
invitation-footer = If you have any questions, contact your system administrator.
invitation-body-text =
    G'day { $name },

    You've been invited to join { $app } by { $by }.

    To complete your account setup and create your password, open this link in your browser:

    { $link }

    A few things to know:
      - This invitation will expire in 7 days.
      - You'll create a password during setup.
      - Choose a strong password with at least 8 characters.
      - If you didn't expect this invitation, you can safely ignore this email.

    -- { $app }

notif-ticket-assigned = [{ $app }] You've been assigned: { $title }
notif-ticket-status-changed = [{ $app }] Status changed: { $title }
notif-comment-added = [{ $app }] New comment on: { $title }
notif-mentioned = [{ $app }] { $actor } mentioned you
notif-ticket-created-requester = [{ $app }] Ticket created: { $title }
notif-doc-page-updated = [{ $app }] Page updated: { $title }
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

# Login + MFA challenge view. en-AU keeps the wording identical
# to en-US/en-GB for this surface — auth screens are conservative
# territory and the AU register doesn't gain anything by diverging.
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

forgot-password-title = Reset Your Password
forgot-password-close-modal = Close modal
forgot-password-intro = Enter your email address and we'll send you a link to reset your password.
forgot-password-email-label = Email Address
forgot-password-email-placeholder = you@example.com
forgot-password-cancel = Cancel
forgot-password-submit = Send Reset Link
forgot-password-submitting = Sending...
forgot-password-error-default = Couldn't send the reset email. Have another go in a moment.
forgot-password-success-title = Check Your Email
forgot-password-success-body = If an account with that email exists, we've sent a password reset link to { $email }
forgot-password-success-important = Important:
forgot-password-success-tip-expiry = The link will expire in <strong>1 hour</strong>
forgot-password-success-tip-spam = Check your spam folder if you don't see it
forgot-password-success-tip-close = You can close this window now
forgot-password-success-done = Done

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

# en-AU greeting uses G'day across all periods — earns its keep
# as the demo divergence.
dashboard-greeting-morning = G'day { $name }.
dashboard-greeting-afternoon = G'day { $name }.
dashboard-greeting-evening = G'day { $name }, evening already?
dashboard-greeting-late-night = G'day { $name }, late night?
dashboard-subtitle = Welcome to your { $app } dashboard
dashboard-edit-button = Edit dashboard
dashboard-guest-fallback = Guest

# Empty states. en-AU follows en-GB spelling (organise);
# casual divergence on the documentation index for the demo.
empty-documentation-grid-title = No documentation yet
empty-documentation-grid-description = Create your first documentation page to get started.
empty-documentation-index-title = Crack open your knowledge base
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
empty-groups-description = Create your first group to organise users
empty-assignment-rules-title = No assignment rules yet
empty-assignment-rules-description = Create your first rule to automatically assign tickets
empty-webhooks-title = No webhooks
empty-webhooks-description = Create a webhook to send events to external services
empty-api-tokens-title = No API tokens
empty-api-tokens-description = Create an API token to enable programmatic access to the API
empty-categories-title = No categories yet
empty-categories-description = Create categories to organise tickets
empty-plugins-installed-title = No plugins installed
empty-plugins-installed-description = Plugins extend { $app } with custom integrations and features. Browse the registry for one-click installs.

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

# en-AU keeps the same wording as en-GB for tickets — the
# empty-state copy is already terse and reads fine in AU register.
ticket-list-empty-no-assigned-message = No tickets assigned to you.
ticket-list-empty-showing-all-active = Showing all active tickets instead.
ticket-list-empty-no-match-title = No tickets match.
ticket-list-empty-no-match-description = Remove some filters to see more.
ticket-list-empty-triage-clear-title = Triage is clear.
ticket-list-empty-triage-clear-description = New tickets awaiting categorisation will appear here.
ticket-list-empty-all-caught-up-title = All sorted.
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
ticket-detail-delete-confirm-body = This can't be undone. The ticket and its history will be removed.
ticket-detail-delete-cancel = Cancel
ticket-detail-delete-confirm = Delete

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
settings-locale-fr-FR = French (France)
settings-locale-nl-NL = Dutch (Netherlands)
settings-timezone-browser-detected = Browser-detected ({ $tz })
settings-timezone-use-device = Use device time zone
settings-timezone-search-placeholder = Search city or offset (e.g. Sydney, UTC+10)
settings-timezone-no-matches = No time zones match that search
settings-save = Save
settings-saving = Saving...
settings-localization-saved = Language and time zone preferences saved
settings-localization-save-failed = Couldn't save preferences

auto-ack-default-template = Your request (#{ $ticket_id }) has been received and is being reviewed by our support team. To add more comments, reply to this email.

inbox-time-just-now = Just now
inbox-time-yesterday = Yesterday at { $time }
inbox-time-weekday = { $day } at { $time }

# First-run admin onboarding.
onboarding-welcome-title = Welcome to Nosdesk
onboarding-welcome-subtitle = Let's get you set up. First step: create your admin account.
onboarding-error-setup-status = Couldn't verify setup status. Give it another go.
onboarding-success-logging-in = Admin account created. Logging you in...
onboarding-success-fallback = Account created. Log in with your credentials.
onboarding-success-fallback-redirect = Account created. Log in to continue.
onboarding-error-setup-failed = Setup failed. Have another crack.
onboarding-error-unexpected = Something went wrong. Please try again.
onboarding-validation-token = Bootstrap token is required
onboarding-validation-name = Administrator name is required
onboarding-validation-email = Email address is required
onboarding-validation-email-format = Enter a valid email address
onboarding-validation-password-length = Password must be at least 8 characters
onboarding-validation-password-mismatch = Passwords don't match
onboarding-token-label = Bootstrap Token
onboarding-token-placeholder = Paste the one-shot token from the server
onboarding-token-hint = Check the server startup logs for a setup URL, or grab it manually with
onboarding-name-label = Administrator Name
onboarding-name-placeholder = Enter your full name
onboarding-email-label = Email Address
onboarding-email-placeholder = Enter your email address
onboarding-password-label = Password
onboarding-password-placeholder = Choose a strong password (8+ characters)
onboarding-confirm-password-label = Confirm Password
onboarding-confirm-password-placeholder = Confirm your password
onboarding-submit = Create Administrator Account
onboarding-submit-loading = Creating Administrator...
onboarding-progress-title = Setting up your account
onboarding-progress-subtitle = Won't take a sec...
onboarding-complete-title = Welcome to Nosdesk
onboarding-complete-subtitle = Your administrator account is ready.
onboarding-migration-title = Migrating from another Nosdesk instance?
onboarding-migration-body-prefix = Create an admin here, then run
onboarding-migration-body-suffix = on the host. The restore replaces the admin with the imported users.
onboarding-security-title = Security Notice
onboarding-security-body = This creates the first administrator account for your Nosdesk installation. Pick a strong password; this account has full system access.

# MFA setup wizard.
mfa-setup-header-default = Finish setting up your account
mfa-setup-header-offer = Add another method?
mfa-setup-header-additional = Add backup method
mfa-setup-subtitle-default = Your account type requires multi-factor auth for security
mfa-setup-subtitle-choose = Choose your preferred authentication method
mfa-setup-subtitle-offer-passkey = Passkeys give you a faster, passwordless sign-in
mfa-setup-subtitle-offer-totp = An authenticator app is a handy backup if you lose your passkey
mfa-setup-subtitle-passkey-additional = Set up a passkey for faster sign-in
mfa-setup-subtitle-totp-additional = Set up an authenticator app as a backup
mfa-setup-totp-name = Authenticator App
mfa-setup-totp-description = Use Google Authenticator, Authy, 1Password or similar to generate time-based codes
mfa-setup-passkey-name = Passkey
mfa-setup-passkey-description = Use biometrics like Face ID, Touch ID, or a hardware security key for passwordless login
mfa-setup-which-title = Which should I choose?
mfa-setup-which-passkey-label = Passkeys
mfa-setup-which-passkey-body = are more secure and convenient, just use your fingerprint or face.
mfa-setup-which-totp-label = Authenticator apps
mfa-setup-which-totp-body = work on any device and don't need biometrics.
mfa-setup-totp-success-title = Authenticator app set up
mfa-setup-totp-success-body = Want to add a passkey for faster, passwordless sign-in?
mfa-setup-passkey-success-title = Passkey created
mfa-setup-passkey-success-body = Want to set up an authenticator app as a backup?
mfa-setup-add-passkey-title = Add a passkey
mfa-setup-add-passkey-description = Use Face ID, Touch ID, or a security key
mfa-setup-add-totp-title = Set up authenticator app
mfa-setup-add-totp-description = Use as a backup if you lose access to your passkey
mfa-setup-skip-now = Skip for now
mfa-setup-back-to-login = Back to Login
mfa-setup-back-skip = Skip
mfa-setup-back-different = Choose different method
mfa-setup-error-session-expired = Session expired. Log in again to set up MFA.
mfa-setup-error-invalid-access = Invalid access. Redirecting to login...

# Password reset.
password-reset-title = Reset your password
password-reset-subtitle = Enter your new password below
password-reset-success-title = Password reset done
password-reset-success-body = Your password has been updated. You can log in with the new one now.
password-reset-success-cta = Go to Login
password-reset-field-new = New password
password-reset-field-new-placeholder = Enter new password
password-reset-field-confirm = Confirm new password
password-reset-field-confirm-placeholder = Confirm new password
password-reset-req-length = At least 8 characters
password-reset-match-yes = Passwords match
password-reset-match-no = Passwords don't match
password-reset-submit = Reset password
password-reset-submit-loading = Resetting password...
password-reset-back-to-login = Back to Login
password-reset-error-no-token = Invalid or missing reset token. Request a new password reset.
password-reset-error-failed = Couldn't reset password. The link may have expired.

# Invitation / guest-ticket accept.
accept-invitation-heading-validating = Hang on a tick…
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
accept-invitation-manual-login = Sign in with the password you just set.
accept-invitation-password-label = Password
accept-invitation-password-placeholder = At least 8 characters
accept-invitation-confirm-label = Confirm password
accept-invitation-confirm-placeholder = Enter it again
accept-invitation-req-length = At least 8 characters
accept-invitation-match-yes = Passwords match
accept-invitation-match-no = Passwords don't match
accept-invitation-show-password = Show password
accept-invitation-hide-password = Hide password
accept-invitation-submit-guest = Confirm & release ticket
accept-invitation-submit-loading-guest = Confirming…
accept-invitation-submit-invitation = Activate account
accept-invitation-submit-loading-invitation = Activating…
accept-invitation-back-to-signin = Back to sign in
accept-invitation-error-missing-token = Invalid or missing confirmation link.
accept-invitation-error-default = This link is invalid or has expired.
accept-invitation-error-validation-failed = Couldn't validate link. Try again in a bit.
accept-invitation-error-submit = Couldn't finish confirmation. The link may have expired.

# Admin: audit log.
admin-audit-title = Audit log
admin-audit-description = Forensic record of who changed what across audited entities. Defaults to the last 7 days and the most recent 50 entries. Refine with the filters below.
admin-audit-filter-entity = Entity
admin-audit-filter-any = Any
admin-audit-filter-entity-id = Entity ID
admin-audit-filter-entity-id-placeholder = e.g. 42
admin-audit-filter-actor = Actor UUID
admin-audit-filter-actor-placeholder = e.g. 0192…
admin-audit-clear-filters = Clear filters
admin-audit-empty-title = No audit entries
admin-audit-empty-description = Either nothing has changed in the selected window, or the filters exclude every row.
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
admin-audit-error-load = Couldn't load audit log
admin-audit-error-load-more = Couldn't load more audit log entries

# Admin: email suppression list.
admin-suppressions-title = Email suppression list
admin-suppressions-description = Addresses we won't attempt to deliver to. Hard bounces (5xx SMTP / 5.x.x enhanced status) land here automatically. Add manually for compliance or complaint-driven blocks. Soft bounces (4xx, transient) never auto-suppress.
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
admin-suppressions-error-load = Couldn't load suppressions
admin-suppressions-error-load-more = Couldn't load more
admin-suppressions-error-add = Couldn't add suppression
admin-suppressions-error-remove = Couldn't remove
admin-suppressions-reason-hard-bounce = hard bounce
admin-suppressions-reason-manual = manual

# Admin: outbound email queue.
admin-email-queue-title = Outbound email queue
admin-email-queue-description = Durable record of every reply we've tried to send. The worker drains pending rows every few seconds. Failed sends retry with exponential backoff. Use this view to dig in when a notification didn't fire.
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
admin-email-queue-empty-description = Either no replies have gone out recently, or the filters exclude every row.
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
admin-email-queue-confirm-message = The email will be marked suppressed and won't be sent.
admin-email-queue-confirm-yes = Cancel send
admin-email-queue-confirm-no = Keep it
admin-email-queue-error-load = Couldn't load email queue
admin-email-queue-error-load-more = Couldn't load more queue entries
admin-email-queue-error-stats = Couldn't load queue stats
admin-email-queue-error-retry = Retry failed
admin-email-queue-error-cancel = Cancel failed

# Admin: workflow states.
admin-workflow-states-title = Workflow
admin-workflow-states-description = Add named ticket states inside the standard workflow categories. Categories are fixed so SLA, dashboards, and automation keep working consistently across teams. New tickets land in the state marked as default.
admin-workflow-states-count-singular = state
admin-workflow-states-count-plural = states
admin-workflow-states-default-badge = Default
admin-workflow-states-make-default = Make default
admin-workflow-states-archive-title = Archive state
admin-workflow-states-archive-disabled-title = Can't archive the default state
admin-workflow-states-archive-confirm = Archive "{ $name }"? Existing tickets will keep this state.
admin-workflow-states-empty-category = No states in this category.
admin-workflow-states-add-placeholder = Add state name
admin-workflow-states-add = Add
admin-workflow-states-error-name-required = Name is required
admin-workflow-states-error-load = Couldn't load workflow states
admin-workflow-states-error-save = Couldn't save state
admin-workflow-states-error-default = Couldn't set default
admin-workflow-states-error-archive = Couldn't archive state
admin-workflow-states-error-promote-first = Promote another state as default before archiving this one.
admin-workflow-states-error-create = Couldn't create state
admin-workflow-states-saved = Saved
admin-workflow-states-default-flash = { $name } is now the default for new tickets
admin-workflow-states-archived-flash = { $name } archived
admin-workflow-states-added-flash = { $name } added to { $category }

# Admin chrome.
admin-back-to-dashboard = Back to Dashboard
admin-heading = Administration
admin-search-placeholder = Search settings...
admin-search-empty = No settings match "{ $query }"
admin-clear-search = Clear search
admin-index-subtitle = Manage system settings, integrations, and workspace config

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
admin-nav-branding-description = Customise the appearance and branding of the application
admin-nav-email-settings-title = Email Configuration
admin-nav-email-settings-description = Configure SMTP settings and send test emails
admin-nav-guest-access-title = Guest Access
admin-nav-guest-access-description = Control what unauthenticated visitors can see and submit
admin-nav-auth-providers-title = Authentication Providers
admin-nav-auth-providers-description = Configure SSO, Microsoft Entra, and local authentication
admin-nav-search-title = Search
admin-nav-search-description = Manage the search index and view indexing statistics
admin-nav-system-settings-title = System Settings
admin-nav-system-settings-description = Manage storage, clean up stale files, and system maintenance
admin-nav-backup-restore-title = Backup & Restore
admin-nav-backup-restore-description = Export and restore system data and attachments

# Admin: System Settings.
admin-system-title = System Settings
admin-system-storage-title = Storage Management
admin-system-storage-description = Remove old user profile images and avatars that aren't needed any more to free up disk space.
admin-system-storage-clean = Clean Up
admin-system-storage-cleaning = Cleaning...
admin-system-storage-confirm-title = Clean up stale images?
admin-system-storage-confirm-message = This can't be undone.
admin-system-storage-confirm-label = Clean up
admin-system-cleanup-success = Cleanup done
admin-system-cleanup-failed = Cleanup failed
admin-system-cleanup-stat-avatars = Avatars:
admin-system-cleanup-stat-banners = Banners:
admin-system-cleanup-stat-thumbnails = Thumbnails:
admin-system-cleanup-stat-checked = Checked:
admin-system-cleanup-stat-errors = Errors:
admin-system-cleanup-view-errors = View Errors ({ $count })
admin-system-cleanup-error-unexpected = Something went wrong while cleaning up images

# Admin: Search Index Management.
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
admin-search-mgmt-stats-error = Couldn't fetch search index statistics
admin-search-mgmt-rebuild-title = Rebuild Search Index
admin-search-mgmt-rebuild-description = Rebuilds the entire search index from the database. Re-indexes all tickets, comments, documentation pages, attachments, devices, and users. Use this if search results are missing or outdated.
admin-search-mgmt-rebuild = Rebuild Index
admin-search-mgmt-rebuilding = Rebuilding...
admin-search-mgmt-rebuild-success = Index rebuilt
admin-search-mgmt-rebuild-failed = Rebuild failed
admin-search-mgmt-rebuild-stat-tickets = Tickets:
admin-search-mgmt-rebuild-stat-comments = Comments:
admin-search-mgmt-rebuild-stat-docs = Docs:
admin-search-mgmt-rebuild-stat-attachments = Attachments:
admin-search-mgmt-rebuild-stat-devices = Devices:
admin-search-mgmt-rebuild-stat-users = Users:
admin-search-mgmt-rebuild-stat-total = Total:
admin-search-mgmt-rebuild-confirm-title = Rebuild the search index?
admin-search-mgmt-rebuild-confirm-message = This may take a few moments depending on the data volume.
admin-search-mgmt-rebuild-confirm-label = Rebuild
admin-search-mgmt-rebuild-error-unexpected = Something went wrong while rebuilding the index

# Admin: Email Configuration.
admin-email-settings-title = Email Configuration
admin-email-settings-description = View email configuration status and send test emails. Email settings live in environment variables.
admin-email-settings-env-notice-prefix = Email settings are configured through environment variables in your
admin-email-settings-env-notice-suffix = file or Docker environment. Use the "Send Test Email" feature to verify your configuration is working.
admin-email-settings-loading = Loading email configuration...
admin-email-settings-service = SMTP Email Service
admin-email-settings-configured = Configured
admin-email-settings-not-configured = Not configured
admin-email-settings-enabled = Enabled
admin-email-settings-server = Server
admin-email-settings-username = Username
admin-email-settings-from-address = From Address
admin-email-settings-password = Password
admin-email-settings-password-not-set = Not set
admin-email-settings-env-vars-label = Env:
admin-email-settings-test-send = Send test:
admin-email-settings-test-placeholder = recipient@example.com
admin-email-settings-test-send-button = Send
admin-email-settings-test-sending = Sending...
admin-email-settings-empty-title = Email is not configured
admin-email-settings-empty-description = Configure email settings in your environment variables to enable email functionality
admin-email-settings-error-load = Couldn't load email configuration
admin-email-settings-error-no-address = Enter an email address
admin-email-settings-error-bad-address = Enter a valid email address
admin-email-settings-test-success = Test email sent
admin-email-settings-error-test = Couldn't send test email

# Admin: Guest Access.
admin-guest-title = Guest Access
admin-guest-description = Control what unauthenticated visitors can see and submit. Everything is off by default.
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
admin-guest-submissions-description = Behaviour for tickets submitted through the public form.
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
admin-guest-error-load = Couldn't load guest settings
admin-guest-error-save = Couldn't save guest settings
admin-guest-saved = Guest access settings saved

# Admin: Data Import hub.
admin-data-import-title = Data Import
admin-data-import-description = Import and synchronise data from external sources
admin-data-import-notice = Data imports may trigger notifications to affected users. Existing records are updated based on matching IDs.
admin-data-import-status-available = Available
admin-data-import-status-coming-soon = Coming Soon
admin-data-import-status-beta = Beta
admin-data-import-microsoft-title = Microsoft Graph
admin-data-import-microsoft-description = Import data from Microsoft 365, including Azure AD, Intune, and other Microsoft services
admin-data-import-csv-title = CSV Import
admin-data-import-csv-description = Import data from CSV files, including devices, users, and other resources
admin-data-import-api-title = API Integrations
admin-data-import-api-description = Connect to third-party APIs to import and synchronise data
admin-data-import-ad-title = Active Directory
admin-data-import-ad-description = Import data from on-prem Active Directory servers

# Admin: Authentication Providers.
admin-auth-providers-title = Authentication Providers
admin-auth-providers-env-notice-prefix = Authentication providers are configured through environment variables in your
admin-auth-providers-env-notice-suffix = file. Use the "Validate Config" button to check if each provider is properly configured.
admin-auth-providers-loading = Loading providers...
admin-auth-providers-default-badge = Default
admin-auth-providers-configured = Configured
admin-auth-providers-not-configured = Not configured
admin-auth-providers-enabled = Enabled
admin-auth-providers-client-id = Client ID
admin-auth-providers-tenant-id = Tenant ID
admin-auth-providers-redirect-uri = Redirect URI
admin-auth-providers-secret = Secret
admin-auth-providers-secret-not-set = Not set
admin-auth-providers-env-label = Env:
admin-auth-providers-empty-title = No authentication providers found
admin-auth-providers-empty-description = Configure authentication providers in your environment variables
admin-auth-providers-error-load = Couldn't load authentication providers
admin-auth-providers-error-validate = Configuration validation failed

# Admin: API Tokens.
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
admin-api-tokens-error-load = Couldn't load API tokens
admin-api-tokens-error-create = Couldn't create token
admin-api-tokens-error-revoke = Couldn't revoke token
admin-api-tokens-error-name-required = Token name is required
admin-api-tokens-error-user-required = Select a user
admin-api-tokens-revoke-success = Token revoked
admin-api-tokens-modal-create-title = Create API Token
admin-api-tokens-modal-name-label = Token Name
admin-api-tokens-modal-name-placeholder = e.g., CI/CD Pipeline
admin-api-tokens-modal-name-hint = A descriptive name to identify this token
admin-api-tokens-modal-user-label = User (acts as)
admin-api-tokens-modal-user-placeholder = Select a user...
admin-api-tokens-modal-user-hint = The token has the same permissions as this user
admin-api-tokens-modal-expiration-label = Expiration
admin-api-tokens-modal-no-expiration-label = No expiration
admin-api-tokens-modal-expires-days-suffix = days
admin-api-tokens-modal-expires-hint = Token expires after { $days } days
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
admin-api-tokens-revoke-confirm-message = Sure you want to revoke the token "{ $name }"?
admin-api-tokens-revoke-warning = This can't be undone. Any systems using this token will lose access.
admin-api-tokens-revoking = Revoking...

# Admin: SLA.
admin-sla-title = SLA
admin-sla-description = Working calendars and SLA policies feed the per-ticket SLA pill.
admin-sla-loading = Loading…
admin-sla-error-load = Couldn't load SLA config
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
admin-sla-policy-delete-confirm = Delete this policy? Tickets currently matching it will lose their SLA pill until another policy matches. This can't be undone.
admin-sla-new-calendar-heading = New calendar
admin-sla-new-policy-heading = New policy
admin-sla-field-name = Name
admin-sla-field-tz = Timezone
admin-sla-field-calendar = Calendar
admin-sla-field-response = Response (minutes)
admin-sla-field-resolution = Resolution (minutes)
admin-sla-field-priority = Priority filter
admin-sla-placeholder-name = AU support hours
admin-sla-placeholder-tz = Australia/Sydney
admin-sla-policy-name-placeholder = Critical incidents
admin-sla-schedule-hint = Schedule defaults to Mon-Fri 9-17. Edit by hand or expand here later.
admin-sla-priority-any = Any
admin-sla-priority-low = low
admin-sla-priority-medium = medium
admin-sla-priority-high = high
admin-sla-workspace-default = Workspace default
admin-sla-create = Create

# Admin: Branding.
admin-branding-title = Branding
admin-branding-description = Customise the appearance and branding of the application.
admin-branding-loading = Loading branding configuration...
admin-branding-general-heading = General Settings
admin-branding-app-name-label = Application Name
admin-branding-app-name-placeholder = Nosdesk
admin-branding-app-name-hint = This name appears in the header and browser tab
admin-branding-primary-color-label = Primary Colour
admin-branding-primary-color-hint = Hex colour code for accent elements (e.g., #2C80FF)
admin-branding-save = Save Settings
admin-branding-saving = Saving...
admin-branding-logo-heading = Logo
admin-branding-logo-dark-label = Dark Theme Logo
admin-branding-logo-light-label = Light Theme Logo (Optional)
admin-branding-logo-upload = Upload Logo
admin-branding-logo-uploading = Uploading...
admin-branding-logo-remove = Remove
admin-branding-logo-formats = PNG, SVG, JPEG, or WebP. Max 2MB.
admin-branding-logo-light-hint = Used when light theme is active. Falls back to the main logo.
admin-branding-favicon-heading = Favicon
admin-branding-favicon-upload = Upload Favicon
admin-branding-favicon-uploading = Uploading...
admin-branding-favicon-formats = ICO, PNG, or SVG. Recommended size: 32x32 or 64x64 pixels.
admin-branding-preview-heading = Preview
admin-branding-primary-color-preview = Primary Colour
admin-branding-configured = Custom branding configured
admin-branding-success-saved = Branding settings saved
admin-branding-success-logo = Logo uploaded
admin-branding-success-logo-light = Light theme logo uploaded
admin-branding-success-favicon = Favicon uploaded
admin-branding-success-removed = { $asset } removed
admin-branding-error-load = Couldn't load branding configuration
admin-branding-error-save = Couldn't save branding settings
admin-branding-error-invalid-file = Invalid file
admin-branding-error-upload-logo = Couldn't upload logo
admin-branding-error-upload-logo-light = Couldn't upload light theme logo
admin-branding-error-upload-favicon = Couldn't upload favicon
admin-branding-error-delete = Couldn't delete { $asset }
admin-branding-asset-logo = Logo
admin-branding-asset-logo-light = Light theme logo
admin-branding-asset-favicon = Favicon
admin-branding-confirm-title = Remove { $asset }?
admin-branding-confirm-message = This removes the uploaded image. You can re-upload at any time, but the previous file isn't recoverable.
admin-branding-confirm-remove = Remove

# Admin: Backup & Restore.
admin-backup-title = Backup & Restore
admin-backup-description = Export and restore system data and attachments
admin-backup-create-heading = Create Backup
admin-backup-create-description = Export all system data and attachments to a ZIP archive
admin-backup-include-sensitive-label = Include sensitive data
admin-backup-include-sensitive-description = Includes passwords, MFA secrets, and authentication tokens (encrypted with a password)
admin-backup-encryption-warning = Sensitive data will be encrypted. Lose the password and the data can't be recovered.
admin-backup-encryption-password-label = Encryption Password
admin-backup-encryption-password-placeholder = Enter encryption password
admin-backup-confirm-password-label = Confirm Password
admin-backup-confirm-password-placeholder = Confirm encryption password
admin-backup-passwords-no-match = Passwords don't match
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
admin-backup-docs-error = Couldn't export documentation. Check the console for details.
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
admin-backup-restore-warning = Restoring will replace existing files. This can't be undone.
admin-backup-restore-button = Restore Files
admin-backup-restoring = Restoring...
admin-backup-cancel = Cancel
admin-backup-restore-not-zip = Select a .zip backup file
admin-backup-upload-error = Couldn't upload backup file
admin-backup-restore-success = Restore done: { $files } files restored. { $message }
admin-backup-restore-error = Restore failed. Check the console for details.
admin-backup-delete-confirm-title = Delete this backup?
admin-backup-delete-confirm-message = The backup file will be permanently removed.
admin-backup-delete-confirm-label = Delete

# Admin: Assignment Rules.
admin-assignment-rules-title = Assignment Rules
admin-assignment-rules-description = Configure automatic ticket assignment based on rules
admin-assignment-rules-new = New Rule
admin-assignment-rules-info = Rules are evaluated in priority order (top to bottom). The first matching rule wins. Tickets with an existing assignee aren't auto-assigned.
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
admin-assignment-rules-error-load = Couldn't load assignment rules
admin-assignment-rules-error-name = Rule name is required
admin-assignment-rules-error-user = Select a target user
admin-assignment-rules-error-group = Select a target group
admin-assignment-rules-error-save = Couldn't save rule
admin-assignment-rules-error-update = Couldn't update rule
admin-assignment-rules-error-delete = Couldn't delete rule
admin-assignment-rules-error-reorder = Couldn't reorder rules
admin-assignment-rules-success-create = Rule created
admin-assignment-rules-success-update = Rule updated
admin-assignment-rules-success-delete = Rule deleted
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
admin-assignment-rules-delete-message = Sure you want to delete the rule "{ $name }"? This can't be undone.
admin-assignment-rules-delete-cancel = Cancel
admin-assignment-rules-delete-confirm = Delete
admin-assignment-rules-deleting = Deleting...

# Admin: Categories (CategoriesManagementView).
admin-categories-title = Categories
admin-categories-description = Manage ticket categories and group visibility
admin-categories-new = New Category
admin-categories-info = Categories with no group restrictions show to everyone. Assign groups to lock them down.
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
admin-categories-modal-color-label = Colour
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
admin-categories-delete-message = Sure you want to delete the category "{ $name }"? Any tickets using it will have their category cleared.
admin-categories-delete-cancel = Cancel
admin-categories-delete-confirm = Delete Category
admin-categories-error-name-required = Category name is required
admin-categories-error-load = Couldn't load categories
admin-categories-error-reorder = Couldn't reorder categories
admin-categories-error-save = Couldn't save category
admin-categories-error-update = Couldn't update category
admin-categories-error-delete = Couldn't delete category
admin-categories-success-create = Category created
admin-categories-success-update = Category updated
admin-categories-success-delete = Category deleted

# Admin: Email channels (ChannelsEmailSettingsView).
admin-channels-email-title = Email Ingestion
admin-channels-email-description = Poll a support mailbox over IMAP and turn inbound messages into tickets. Tech replies get relayed back through the same thread.
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
admin-channels-email-field-port-hint = 993 for IMAPS. 143 needs STARTTLS (not supported yet).
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
admin-channels-email-delete-message = Tickets already opened from it stay intact, but no new messages will be ingested. Can't be undone.
admin-channels-email-delete-confirm = Delete channel
admin-channels-email-relative-seconds = { $count }s ago
admin-channels-email-relative-minutes = { $count }m ago
admin-channels-email-relative-hours = { $count }h ago
admin-channels-email-relative-days = { $count }d ago
admin-channels-email-error-load = Couldn't load email channel
admin-channels-email-success-update = Channel updated
admin-channels-email-success-create = Channel created
admin-channels-email-success-password-removed = Password removed
admin-channels-email-success-delete = Channel deleted

# Admin: Microsoft Graph (data import)
admin-msgraph-back = Back to Data Import
admin-msgraph-title = Microsoft Graph
admin-msgraph-subtitle = Manage data sync from Microsoft 365 services
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
admin-msgraph-last-synced = Last synced:
admin-msgraph-missing-config = Missing required configuration:
admin-msgraph-env-label = Env:
admin-msgraph-progress-title = Syncing
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
admin-msgraph-last-sync-title = Last Sync
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
admin-msgraph-modal-description = Pick the data you want to import from Microsoft Graph:
admin-msgraph-entity-users-name = Users
admin-msgraph-entity-users-description = Import user accounts and profiles from Microsoft Entra ID
admin-msgraph-entity-devices-name = Devices
admin-msgraph-entity-devices-description = Import managed devices from Microsoft Intune with user assignments
admin-msgraph-entity-groups-name = Groups
admin-msgraph-entity-groups-description = Import security and distribution groups from Microsoft Entra ID
admin-msgraph-modal-info = Sync pulls the latest data from Microsoft services. Can take a few minutes depending on how much data you have.
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
admin-msgraph-error-validate-config = Couldn't validate configuration
admin-msgraph-error-fetch-status = Couldn't fetch connection status
admin-msgraph-error-start-sync = Couldn't start sync
admin-msgraph-error-cancel-sync = Couldn't cancel sync
admin-msgraph-success-sync-started = Sync started
admin-msgraph-success-cancel-requested = Sync cancellation requested

# Admin: Plugin registry (browse and install)
admin-plugins-registry-back = Installed plugins
admin-plugins-registry-title = Plugin registry
admin-plugins-registry-subtitle-before = Browse and install plugins published to
admin-plugins-registry-subtitle-after = . Signatures get verified against the Nosdesk root key before any bundle runs.
admin-plugins-registry-refresh = Refresh
admin-plugins-registry-refreshing = Refreshing
admin-plugins-registry-loading = Loading registry...
admin-plugins-registry-disabled-title = Registry sync is off
admin-plugins-registry-disabled-description-sideload = This instance has NOSDESK_REGISTRY_URL set to empty, so it isn't pulling the published plugin catalogue. You can still sideload a signed zip.
admin-plugins-registry-disabled-description-cli = This instance has NOSDESK_REGISTRY_URL set to empty, so it isn't pulling the published plugin catalogue. Use the CLI to install local-signed plugins.
admin-plugins-registry-disabled-action = Sideload signed zip
admin-plugins-registry-pending-title = Registry is syncing
admin-plugins-registry-pending-description = This instance is fetching the published plugin catalogue. Usually wraps up within a few seconds of boot.
admin-plugins-registry-failed-title = Registry sync failed
admin-plugins-registry-failed-description = { $reason }. Have another go, or wait for the next scheduled attempt.
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
admin-plugins-registry-community-warning-body = Nosdesk doesn't vouch for the safety of community plugins past verifying the publisher's signature. Have a squiz at the source before trusting it with your data.
admin-plugins-registry-field-publisher = Publisher
admin-plugins-registry-field-fingerprint = Fingerprint
admin-plugins-registry-field-version = Version
admin-plugins-registry-type-to-confirm-before = Type
admin-plugins-registry-type-to-confirm-after = to confirm
admin-plugins-registry-cancel = Cancel
admin-plugins-registry-error-load = Couldn't load the registry.
admin-plugins-registry-error-refresh = Couldn't retry the registry sync.
admin-plugins-registry-error-confirm-name = Type the plugin name exactly to confirm install.
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
admin-webhooks-create = Create webhook
admin-webhooks-create-short = Create
admin-webhooks-loading = Loading webhooks...
admin-webhooks-section-active = Active webhooks
admin-webhooks-section-disabled = Disabled webhooks
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
admin-webhooks-action-send-test = Send a test event
admin-webhooks-action-view-deliveries = View deliveries
admin-webhooks-action-edit = Edit webhook
admin-webhooks-action-delete = Delete webhook
admin-webhooks-modal-create-title = Create webhook
admin-webhooks-modal-edit-title = Edit webhook
admin-webhooks-modal-secret-title = Webhook created
admin-webhooks-modal-regenerate-title = Regenerate secret
admin-webhooks-modal-delete-title = Delete webhook
admin-webhooks-modal-deliveries-title = Delivery history - { $name }
admin-webhooks-form-name-label = Name
admin-webhooks-form-name-placeholder = e.g. Slack notifications
admin-webhooks-form-url-label = Payload URL
admin-webhooks-form-url-placeholder = https://example.com/webhook
admin-webhooks-form-url-hint = POST requests go to this URL
admin-webhooks-form-events-label = Events
admin-webhooks-form-events-hint = Pick which events trigger this webhook
admin-webhooks-form-events-count = { $selected }/{ $total }
admin-webhooks-form-headers-label = Custom headers
admin-webhooks-form-headers-add = + Add header
admin-webhooks-form-headers-name-placeholder = Header name
admin-webhooks-form-headers-value-placeholder = Value
admin-webhooks-form-headers-empty = No custom headers
admin-webhooks-form-enabled-label = Enabled
admin-webhooks-form-enabled-description = Webhook receives events while enabled
admin-webhooks-form-secret-label = Secret
admin-webhooks-form-secret-regenerate = Regenerate
admin-webhooks-form-cancel = Cancel
admin-webhooks-form-create = Create webhook
admin-webhooks-form-creating = Creating...
admin-webhooks-form-save = Save changes
admin-webhooks-form-saving = Saving...
admin-webhooks-secret-warning = Copy this secret now, you won't see it again!
admin-webhooks-secret-helper-before = Use this secret to verify webhook signatures via the
admin-webhooks-secret-helper-after = header
admin-webhooks-secret-copy = Copy to clipboard
admin-webhooks-secret-copied = Copied!
admin-webhooks-secret-done = Done
admin-webhooks-regenerate-question = Sure you want to regenerate the secret for { $name }?
admin-webhooks-regenerate-warning = The current secret will be invalidated. You'll need to update your integration with the new secret.
admin-webhooks-regenerate-confirm = Regenerate
admin-webhooks-regenerate-running = Regenerating...
admin-webhooks-delete-question = Sure you want to delete the webhook { $name }?
admin-webhooks-delete-warning = This can't be undone. All delivery history will be lost.
admin-webhooks-delete-confirm = Delete webhook
admin-webhooks-delete-running = Deleting...
admin-webhooks-deliveries-loading = Loading deliveries...
admin-webhooks-deliveries-empty-title = No deliveries yet
admin-webhooks-deliveries-empty-description = Deliveries show up here once events fire
admin-webhooks-deliveries-status-error = Error
admin-webhooks-deliveries-status-pending = Pending
admin-webhooks-deliveries-attempt = Attempt { $number }
admin-webhooks-deliveries-duration = { $ms }ms
admin-webhooks-deliveries-close = Close
admin-webhooks-error-name-required = Webhook name is required
admin-webhooks-error-url-required = URL is required
admin-webhooks-error-event-required = Pick at least one event
admin-webhooks-error-load = Couldn't load webhooks
admin-webhooks-error-create = Couldn't create webhook
admin-webhooks-error-update = Couldn't update webhook
admin-webhooks-error-delete = Couldn't delete webhook
admin-webhooks-error-test = Couldn't send test event
admin-webhooks-error-regenerate = Couldn't regenerate secret
admin-webhooks-success-update = Webhook updated
admin-webhooks-success-delete = Webhook deleted
admin-webhooks-success-test = Test event sent to webhook
admin-webhooks-success-regenerate = Secret regenerated, check webhook deliveries for the new signature
admin-webhooks-category-tickets = Tickets
admin-webhooks-category-comments = Comments
admin-webhooks-category-attachments = Attachments
admin-webhooks-category-devices = Devices
admin-webhooks-category-projects = Projects
admin-webhooks-category-documentation = Documentation
admin-webhooks-category-users = Users
admin-webhooks-event-ticket-created = Ticket created
admin-webhooks-event-ticket-updated = Ticket updated
admin-webhooks-event-ticket-deleted = Ticket deleted
admin-webhooks-event-ticket-linked = Ticket linked
admin-webhooks-event-ticket-unlinked = Ticket unlinked
admin-webhooks-event-comment-added = Comment added
admin-webhooks-event-comment-deleted = Comment deleted
admin-webhooks-event-attachment-added = Attachment added
admin-webhooks-event-attachment-deleted = Attachment deleted
admin-webhooks-event-device-linked = Device linked
admin-webhooks-event-device-unlinked = Device unlinked
admin-webhooks-event-device-updated = Device updated
admin-webhooks-event-project-assigned = Project assigned
admin-webhooks-event-project-unassigned = Project unassigned
admin-webhooks-event-documentation-updated = Documentation updated
admin-webhooks-event-user-created = User created
admin-webhooks-event-user-updated = User updated
admin-webhooks-event-user-deleted = User deleted

# Users list (UsersListView): people directory with role filter,
# bulk role change, and bulk delete.
user-mgmt-search-placeholder = Search users...
user-mgmt-item-label = user
user-mgmt-filter-all-roles = All roles
user-mgmt-role-admin = Admin
user-mgmt-role-technician = Technician
user-mgmt-role-user = User
user-mgmt-column-user = User
user-mgmt-column-role = Role
user-mgmt-column-tickets = Tickets
user-mgmt-column-devices = Devices
user-mgmt-column-joined = Joined
user-mgmt-invite-action = Invite user
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
    [one] This will permanently delete one user. This can't be undone.
   *[other] This will permanently delete { $count } users. This can't be undone.
}
user-mgmt-bulk-action-error = Couldn't do that bulk action. Give it another go.
user-mgmt-role-modal-title = Set role
user-mgmt-role-modal-body = { $count ->
    [one] Update role for { $count } user
   *[other] Update role for { $count } users
   }

# User profile (UserProfileView): single user detail + create-user
# form, devices/groups panels, assigned & requested ticket panes.
user-profile-document-title = { $name }'s profile | Nosdesk
user-profile-back-to-users = Back to users
user-profile-action-profile-settings = Profile settings
user-profile-action-user-settings = User settings
user-profile-create-title = Create new user
user-profile-create-subtitle = Add a new user to your organisation
user-profile-section-basic-info = Basic info
user-profile-field-name = Full name
user-profile-field-name-placeholder = Enter full name
user-profile-field-email = Email address
user-profile-field-email-placeholder = user@example.com
user-profile-field-role = Role
user-profile-field-role-placeholder = Pick a role
user-profile-role-user = User
user-profile-role-technician = Technician
user-profile-role-admin = Admin
user-profile-field-pronouns = Pronouns
user-profile-field-pronouns-placeholder = e.g. he/him, she/her, they/them
user-profile-section-account-setup = Account setup
user-profile-smtp-warning-title = Email not configured
user-profile-smtp-warning-body = You'll need to set a password manually since email invitations aren't available.
user-profile-setup-method = Setup method
user-profile-setup-invite-title = Send invitation email
user-profile-setup-invite-body = User will get an email with a secure link to set their own password
user-profile-setup-password-title = Set password manually
user-profile-setup-password-body = Create a password for the user now and share it with them securely
user-profile-field-password = Password
user-profile-field-password-placeholder = Minimum 8 characters
user-profile-field-confirm-password = Confirm password
user-profile-field-confirm-password-placeholder = Re-enter password
user-profile-passwords-match = Passwords match
user-profile-passwords-no-match = Passwords don't match
user-profile-required-note = Required fields
user-profile-action-cancel = Cancel
user-profile-action-create = Create user
user-profile-action-creating = Creating...
user-profile-devices-title = Devices
user-profile-devices-empty = No devices
user-profile-device-manufacturer-unknown = Unknown
user-profile-device-last-updated = Last updated { $when }
user-profile-groups-title = Groups
user-profile-not-found = User not found
user-profile-error-no-create-permission = You don't have permission to create users
user-profile-error-missing-id = User ID is missing
user-profile-error-password-too-short = Password needs to be at least 8 characters long
user-profile-error-passwords-mismatch = Passwords don't match
user-profile-error-created-no-uuid = User created but navigation failed. Head to the users list.
user-profile-error-save-generic = Couldn't save user. Give it another go.
user-profile-error-load = Couldn't load user profile
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
groups-mgmt-field-color = Colour
groups-mgmt-action-cancel = Cancel
groups-mgmt-action-create = Create Group
groups-mgmt-modal-delete-title = Delete Group
groups-mgmt-delete-confirm-body = Sure you want to delete the group <strong class="text-primary">{ $name }</strong>? This removes all member associations but won't delete the users themselves.
groups-mgmt-action-delete-confirm = Delete Group
groups-mgmt-error-name-required = Group name is required
groups-mgmt-error-load = Couldn't load groups
groups-mgmt-error-create = Couldn't create group
groups-mgmt-error-delete = Couldn't delete group
groups-mgmt-success-created = Group created
groups-mgmt-success-deleted = Group deleted

# Group detail (GroupDetailView): per-group page showing
# sync status, members, devices, and creation metadata.
group-detail-error-invalid-id = Invalid group ID
group-detail-error-load = Couldn't load group details
group-detail-sync-source-microsoft = Microsoft Entra ID
group-detail-type-security = Security
group-detail-type-mail-enabled = Mail-enabled
group-detail-type-standard = Standard
group-detail-synced-from = Synced from { $source }
group-detail-action-configure = Configure
group-detail-section-information = Group info
group-detail-field-type = Type
group-detail-field-sync-source = Sync source
group-detail-field-last-synced = Last synced
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
    [one] This will permanently delete one device. This can't be undone.
   *[other] This will permanently delete { $count } devices. This can't be undone.
}
devices-list-bulk-action-error = Couldn't delete devices. Please try again.

# Device detail (DeviceView): per-device page covering name, hostname,
# hardware identifiers, warranty fields, primary user, Microsoft Intune
# integration, and the unmanage / create flows.
device-detail-back-to-ticket = Back to Ticket #{ $id }
device-detail-back-to-devices = Go back
device-detail-readonly = Read-only
device-detail-delete-item-name = Device
device-detail-error-invalid-id = Invalid device ID
device-detail-error-load = Couldn't load device details
device-detail-error-create = Couldn't create device. Please try again.
device-detail-error-delete = Couldn't delete device. Please try again.
device-detail-error-unmanage = Couldn't unmanage device. Please try again.
device-detail-section-details = Device details
device-detail-field-name = Name
device-detail-field-name-placeholder-create = Enter device name
device-detail-field-name-placeholder-edit = Enter name...
device-detail-field-hostname = Hostname
device-detail-field-hostname-placeholder-create = Enter hostname
device-detail-field-hostname-placeholder-edit = Enter hostname...
device-detail-field-serial = Serial number
device-detail-field-serial-placeholder-create = Enter serial number
device-detail-field-serial-placeholder-edit = Enter serial number...
device-detail-field-manufacturer = Manufacturer
device-detail-field-manufacturer-placeholder-create = e.g., Dell, HP, Apple
device-detail-field-manufacturer-placeholder-edit = Enter manufacturer...
device-detail-field-model = Model
device-detail-field-model-placeholder-create = Enter device model
device-detail-field-model-placeholder-edit = Enter model...
device-detail-field-warranty-status = Warranty status
device-detail-field-warranty-start = Warranty start
device-detail-field-warranty-end = Warranty end
device-detail-field-purchase-date = Purchase date
device-detail-field-asset-tag = Asset tag
device-detail-field-asset-tag-placeholder-create = Enter asset tag
device-detail-field-asset-tag-placeholder-edit = Enter asset tag...
device-detail-warranty-active = Active
device-detail-warranty-warning = Warning
device-detail-warranty-expired = Expired
device-detail-warranty-unknown = Unknown
device-detail-section-primary-user = Primary user
device-detail-no-user-assigned = No user assigned to this device
device-detail-action-assign-user = Assign user
device-detail-action-change-user = Change user
device-detail-section-device-information = Device info
device-detail-field-device-id = Device ID
device-detail-field-created = Created
device-detail-field-last-updated = Last updated
device-detail-manually-managed = Manually managed
device-detail-manually-managed-description = This device was created and is managed manually in Nosdesk
device-detail-section-microsoft-integration = Microsoft integration
device-detail-field-last-intune-check-in = Last Intune check-in
device-detail-action-view-in-intune = View in Intune
device-detail-action-view-in-entra = View in Entra
device-detail-action-unmanage = Unmanage from Intune/Entra
device-detail-action-unmanage-processing = Processing...
device-detail-action-unmanage-title = Remove from Microsoft Intune/Entra management
device-detail-unmanage-conversion-note = This will convert the device to manual management
device-detail-tech-details-show = Show technical details
device-detail-tech-details-hide = Hide technical details
device-detail-field-intune-id = Intune ID
device-detail-field-entra-id = Entra ID
device-detail-not-managed-by-intune = This device isn't managed by Microsoft Intune
device-detail-action-cancel = Cancel
device-detail-action-create = Create device
device-detail-action-create-processing = Creating...
device-detail-not-found = Device not found
device-detail-unmanage-modal-title = Unmanage device
device-detail-unmanage-heading = Unmanage from Microsoft
device-detail-unmanage-confirm-body = Sure you want to unmanage <strong class="text-primary">{ $name }</strong> from Microsoft Intune/Entra?
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
project-cycles-confirm-complete = Wrap up this cycle? The snapshot locks in once you do.
project-cycles-confirm-archive = Archive this cycle?
project-cycles-create-title = New cycle
project-cycles-field-name = Name
project-cycles-field-start = Start
project-cycles-field-end = End
project-cycles-name-placeholder = e.g. Sprint 14
project-cycles-create-submit = Create
project-cycles-all-title = All cycles
project-cycles-empty-prefix = No cycles yet. Hit
project-cycles-empty-cta = New cycle
project-cycles-empty-suffix = to kick one off.
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
workspace-cycles-error-fallback = Couldn't load cycles
workspace-cycles-empty-title = No cycles yet.
workspace-cycles-empty-hint = Pop into a project and kick one off from the Cycles drawer.
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
cycle-detail-error-fallback = Couldn't load cycle
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
docs-index-no-recent-activity = Nothing happening lately.
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
docs-drafts-description = Pages not yet sorted into a collection
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
docs-gaps-impact-tooltip = { $count } { $label } showing demand for this doc
docs-gaps-signal-count = { $count ->
    [one] { $count } signal
   *[other] { $count } signals
   }
docs-gaps-select-prompt = Pick a gap from the list to see its evidence.
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
doc-detail-needs-verification = Needs a check
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
doc-detail-not-found-body = The document you're after doesn't exist or has been moved.
doc-detail-not-found-link = Go to Documentation Home
doc-detail-toast-deleting = Deleting document
doc-detail-toast-deleted = Document deleted
doc-detail-toast-delete-error = Couldn't delete document
doc-detail-duplicate-suffix = { $title } (copy)
doc-detail-ticket-note-title = Notes for Ticket #{ $id }
doc-detail-ticket-note-description = Documentation for ticket { $title }
doc-detail-ticket-note-author-system = System

# Asset planner (AssetPlannerView): rollout-planning kanban for
# devices, grouped by OS family, warranty bucket, or compliance
# state. Covers the header, sidebar filters, group columns, and
# device card chips.
asset-planner-title = Assets
asset-planner-subtitle = Plan rollouts by OS, warranty, or compliance.
asset-planner-search-placeholder = Search by name, hostname, model…
asset-planner-group-by = Group by
asset-planner-axis-os = OS family
asset-planner-axis-warranty = Warranty
asset-planner-axis-compliance = Compliance
asset-planner-loading = Loading assets…
asset-planner-load-error = Couldn't load assets
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
collection-not-found-description = This collection might have been moved or deleted.
collection-badge-system = System
collection-badge-restricted = Restricted
collection-badge-public = Public
collection-overview-heading = Overview
collection-overview-placeholder = Jot down an overview for this collection...
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
collection-delete-message = Pages in this collection won't be deleted.
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
csv-import-requirements-headers = The first row needs column headers matching the expected fields
csv-import-requirements-required = Required fields can't be empty
csv-import-requirements-date-format = Date fields should use the format YYYY-MM-DD
csv-import-requirements-max-size = Maximum file size: 10MB
csv-import-notes-heading = Important Notes
csv-import-notes-updates = Existing records get updated if they share a unique identifier (like email or ID)
csv-import-notes-validation = Data validation runs before import, records with invalid data will be skipped
csv-import-notes-duration = For large imports, the process can take a few minutes to finish
csv-import-notes-templates = Download and use our template files to keep formatting right
csv-import-templates-heading = Available Templates
csv-import-templates-intro = Use these templates as a starting point for your CSV imports
csv-import-template-users-name = Users Template
csv-import-template-users-description = Import user accounts with roles and contact info
csv-import-template-devices-name = Devices Template
csv-import-template-devices-description = Import devices with hardware details and ownership info
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
csv-import-modal-templates-intro = Download our CSV templates to make sure your data is formatted correctly for import.
csv-import-modal-fields-count = { $count ->
    [one] { $count } field
   *[other] { $count } fields
   }
csv-import-modal-close = Close
csv-import-error-no-file = Pick a file to import
csv-import-error-failed = Import failed
csv-import-error-generic = Couldn't import data
csv-import-success-completed = Import completed successfully
csv-import-toast-template-downloaded = { $type } template downloaded

# Error page (ErrorView)
error-page-default-code = 404
error-page-default-message = Page not found
error-page-description = The page you're after doesn't exist, or you may not have access to it.
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
pdf-viewer-error-title = Couldn't load PDF
pdf-viewer-error-go-back = Go back
pdf-viewer-error-no-source = No PDF source provided
pdf-viewer-error-failed = Couldn't load PDF
pdf-viewer-error-failed-with-reason = Couldn't load PDF: { $reason }
pdf-viewer-error-unknown = Unknown error

# Settings: MFA (MFASettings) - two-factor authentication setup, verify, backup codes, disable
settings-mfa-title = Two-Factor Authentication
settings-mfa-title-success = Setup Complete!
settings-mfa-toggle-label = Enable Two-Factor Authentication
settings-mfa-toggle-description-enabled = Your account is protected with 2FA
settings-mfa-toggle-description-disabled = Lock your account down with an authenticator app
settings-mfa-admin-status-enabled = Enabled
settings-mfa-admin-status-disabled = Not enabled
settings-mfa-admin-backup-codes-generated = · Backup codes generated
settings-mfa-admin-disable = Disable
settings-mfa-admin-disabling = Disabling...
settings-mfa-admin-note = MFA setup needs the account owner's authenticator app.
settings-mfa-admin-disable-success = Two-factor authentication has been disabled for this user
settings-mfa-admin-disable-error = Couldn't disable MFA
settings-mfa-admin-load-error = Couldn't load MFA status for this user
settings-mfa-setup-init-error = Couldn't start MFA setup
settings-mfa-setup-not-ready = MFA setup isn't ready yet
settings-mfa-manual-toggle = Can't scan? Enter the code manually
settings-mfa-manual-instructions = Enter this secret key in your authenticator app:
settings-mfa-copy-button = Copy
settings-mfa-copied-button = Copied!
settings-mfa-copy-tooltip = Copy to clipboard
settings-mfa-copied-tooltip = Copied to clipboard!
settings-mfa-copy-error = Couldn't copy to clipboard
settings-mfa-verify-heading = Enter Verification Code
settings-mfa-verify-instructions = Enter the 6-digit code from your authenticator app:
settings-mfa-verify-aria-label = MFA verification code
settings-mfa-verify-button = Verify
settings-mfa-verifying-button = Verifying...
settings-mfa-verify-invalid-length = Enter a valid 6-digit code
settings-mfa-verify-missing-secret = MFA secret is missing. Start the setup process again.
settings-mfa-verify-invalid-code = That code didn't work. Have another go.
settings-mfa-verify-incomplete-login = MFA's on but the login response came back incomplete
settings-mfa-qr-alt = MFA QR Code
settings-mfa-verifying-heading = Verifying Code
settings-mfa-verifying-message = Hang on while we check your authenticator code...
settings-mfa-disable-password-prompt = Enter your password to disable MFA:
settings-mfa-backup-codes-heading = Backup Codes
settings-mfa-backup-codes-description = Stash these backup codes somewhere safe. You can use them to get into your account if you lose your authenticator device.
settings-mfa-backup-codes-download = Download
settings-mfa-backup-codes-download-tooltip = Download backup codes as a text file
settings-mfa-backup-codes-download-success = Backup codes downloaded
settings-mfa-backup-codes-download-error = Couldn't download backup codes
settings-mfa-backup-file-title = Nosdesk Backup Codes
settings-mfa-backup-file-warning = IMPORTANT: Stash these backup codes somewhere safe.
settings-mfa-backup-file-usage = Each code can only be used once to get into your account if you lose your authenticator device.
settings-mfa-backup-file-codes-heading = Backup Codes:
settings-mfa-backup-file-generated = Generated on: { $date }
settings-mfa-success-heading = Two-Factor Authentication Enabled!
settings-mfa-success-message = Your account's now locked down with 2FA. You'll need to enter a code from your authenticator app when you sign in.
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
settings-auth-methods-link-success = { $provider } account linked
settings-auth-methods-link-error = Couldn't link { $provider } account
settings-auth-methods-remove-success = Authentication method removed
settings-auth-methods-remove-error = Couldn't remove authentication method
settings-auth-methods-sessions-section-title = Active Sessions
settings-auth-methods-sessions-revoke-all = Revoke All Others
settings-auth-methods-sessions-unknown-device = Unknown Device
settings-auth-methods-sessions-unknown-location = Unknown location
settings-auth-methods-sessions-current-badge = Current
settings-auth-methods-sessions-last-active = { $location } • Last active { $date }
settings-auth-methods-sessions-revoke = Revoke
settings-auth-methods-sessions-revoke-success = Session revoked
settings-auth-methods-sessions-revoke-error = Couldn't revoke session
settings-auth-methods-sessions-revoke-all-success = All other sessions revoked
settings-auth-methods-sessions-revoke-all-error = Couldn't revoke sessions
settings-auth-methods-sessions-load-error = Couldn't load active sessions

# Shared common chrome.
common-modal-close = Close modal

# Editor: toolbar (CollaborativeEditor)
editor-toolbar-text-style = Text style
editor-toolbar-bold = Bold
editor-toolbar-italic = Italic
editor-toolbar-bullet-list = Bullet list
editor-toolbar-numbered-list = Numbered list
editor-toolbar-insert = Insert
editor-toolbar-undo = Undo
editor-toolbar-redo = Redo
editor-toolbar-revision-history = Revision history
editor-toolbar-editing-with = Editing with:
editor-toolbar-connection-connecting = Connecting...
editor-toolbar-connection-disconnected = Disconnected
editor-toolbar-user-title = { $name }
editor-toolbar-user-title-uuid = { $name } (UUID: { $uuid })

# Editor: text style menu (CollaborativeEditor)
editor-type-menu-plain = Plain
editor-type-menu-heading-1 = Heading 1
editor-type-menu-heading-2 = Heading 2
editor-type-menu-heading-3 = Heading 3
editor-type-menu-blockquote = Blockquote
editor-type-menu-code-block = Code block

# Editor: insert menu (CollaborativeEditor)
editor-insert-menu-bullet-list = Bullet list
editor-insert-menu-numbered-list = Numbered list
editor-insert-menu-blockquote = Blockquote
editor-insert-menu-code-block = Code block
editor-insert-menu-link = Link
editor-insert-menu-embed-document = Embed document

# Editor: code block language prompt (CollaborativeEditor)
editor-code-block-language-prompt = Language for syntax highlighting (optional):

# Editor: mention dropdown (CollaborativeEditor)
editor-mention-searching = Searching for "{ $query }"
editor-mention-no-results = No users found
editor-mention-hint-navigate = Navigate
editor-mention-hint-select = Select
editor-mention-hint-close = Close

# Editor: link tooltip (LinkTooltip)
editor-link-tooltip-placeholder = Enter URL...
editor-link-tooltip-apply = Apply
editor-link-tooltip-cancel = Cancel
editor-link-tooltip-edit = Edit link
editor-link-tooltip-remove = Remove link

# Editor: document picker (DocumentPicker)
editor-doc-picker-title = Embed document
editor-doc-picker-close = Close
editor-doc-picker-search-placeholder = Search docs...
editor-doc-picker-empty = No docs found.

# Editor: revision history panel (RevisionHistory)
editor-revision-history-title = Revision history

# Editor: revision list (RevisionList)
editor-revisions-empty-title = No revisions yet
editor-revisions-empty-hint = Revisions get created when you make changes
editor-revisions-current-version = Current version
editor-revisions-by = By:
editor-revisions-more-contributors = +{ $count }
editor-revisions-word-count = { $count } { $count ->
    [one] word
   *[other] words
  }
editor-revisions-restore-button = Restore this version
editor-revisions-restoring = Restoring...
editor-revisions-unknown-user = Unknown
editor-revisions-load-error = Couldn't load revisions
editor-revisions-restore-error = Couldn't restore revision
editor-revisions-just-now = Just now
editor-revisions-minutes-ago = { $minutes }m ago
editor-revisions-hours-ago = { $hours }h ago
editor-revisions-days-ago = { $days }d ago
editor-revisions-confirm-title = Restore revision?
editor-revisions-confirm-body = This rolls the ticket back to revision { $revision } and replaces the current content with the selected one.
editor-revisions-confirm-note = Heads up: a new revision gets created so you can always undo this.
editor-revisions-confirm-cancel = Cancel
editor-revisions-confirm-restore = Restore

# Ticket media: attachment preview (AttachmentPreview)
ticket-media-attachment-voice-message = Voice Message
ticket-media-attachment-file-fallback = File
ticket-media-attachment-pdf-document = PDF Document.{ $ext }
ticket-media-attachment-video = Video.{ $ext }
ticket-media-attachment-audio = Audio.{ $ext }
ticket-media-attachment-image = Image.{ $ext }
ticket-media-attachment-file = File.{ $ext }
ticket-media-attachment-download = Download attachment
ticket-media-attachment-download-image = Download image
ticket-media-attachment-download-animated = Download animated image
ticket-media-attachment-download-pdf = Download PDF
ticket-media-attachment-delete-audio = Delete audio
ticket-media-attachment-delete-video = Delete video
ticket-media-attachment-delete-image = Delete image
ticket-media-attachment-delete-pdf = Delete PDF
ticket-media-attachment-delete-file = Delete file
ticket-media-attachment-format-unsupported = Your browser can't show this image format
ticket-media-attachment-loading-pdf = Loading PDF
ticket-media-attachment-animated-badge = ANIMATED
ticket-media-attachment-cancel = Cancel
ticket-media-attachment-submit-video = Submit Video
ticket-media-attachment-preview-title-animated = Animated Image Preview
ticket-media-attachment-preview-title-image = Image Preview

# Ticket media: audio player (AudioPlayer)
ticket-media-audio-play = Play
ticket-media-audio-pause = Pause
ticket-media-audio-loading = Loading...
ticket-media-audio-transcription = Transcription

# Ticket media: voice recorder (VoiceRecorder)
ticket-media-voice-recording = Recording
ticket-media-voice-cancel = Cancel
ticket-media-voice-stop = Stop Recording
ticket-media-voice-mic-error = Couldn't reach the mic. Check your permissions.

# Ticket media: video player (VideoPlayer)
ticket-media-video-play = Play
ticket-media-video-pause = Pause
ticket-media-video-mute = Mute
ticket-media-video-unmute = Unmute
ticket-media-video-fullscreen-enter = Go fullscreen
ticket-media-video-fullscreen-exit = Exit fullscreen

# Ticket media: PDF viewer (PDFViewer)
ticket-media-pdf-aria = PDF viewer
ticket-media-pdf-loading = Loading PDF...
ticket-media-pdf-zoom-out = Zoom Out
ticket-media-pdf-zoom-out-aria = Zoom out
ticket-media-pdf-zoom-in = Zoom In
ticket-media-pdf-zoom-in-aria = Zoom in
ticket-media-pdf-fit-width = Fit to Width
ticket-media-pdf-fit-width-aria = Fit to width
ticket-media-pdf-fullscreen = Fullscreen
ticket-media-pdf-fullscreen-aria = Open fullscreen
ticket-media-pdf-download = Download PDF
ticket-media-pdf-download-aria = Download PDF

# Ticket media: generic file preview (FilePreview)
ticket-media-file-fallback = File
ticket-media-file-pdf = PDF Document.{ $ext }
ticket-media-file-word = Word Document.{ $ext }
ticket-media-file-excel = Excel Spreadsheet.{ $ext }
ticket-media-file-powerpoint = Presentation.{ $ext }
ticket-media-file-image = Image.{ $ext }
ticket-media-file-archive = Archive.{ $ext }
ticket-media-file-text = Text Document.{ $ext }
ticket-media-file-generic = File.{ $ext }
ticket-media-file-delete = Delete file
ticket-media-file-download = Download
ticket-media-file-thumbnail-error = Couldn't make a thumbnail
ticket-media-file-image-error = Couldn't load the image
ticket-media-file-animated-badge = ANIMATED

# Ticket picker: user picker (UserPicker)
ticket-picker-user-placeholder-assignee = Assign to...
ticket-picker-user-placeholder-requester = Find a user...
ticket-picker-user-search-staff = Search staff...
ticket-picker-user-search-users = Search users...
ticket-picker-user-sheet-title-assignee = Assign to
ticket-picker-user-sheet-title-requester = Find user
ticket-picker-user-listbox-assignees = Assignable users
ticket-picker-user-listbox-users = Users
ticket-picker-user-loading-assignee = Loading assignees
ticket-picker-user-loading-requester = Loading requesters
ticket-picker-user-view-profile = View { $name }'s profile
ticket-picker-user-clear = Clear selection
ticket-picker-user-empty-assignees = No assignable users yet.
ticket-picker-user-empty-users = No users found.
ticket-picker-user-empty-search = No users match "{ $query }"
ticket-picker-user-section-selected-assignee = Currently assigned
ticket-picker-user-section-selected-requester = Current requester
ticket-picker-user-section-you = You
ticket-picker-user-section-recent = Recent
ticket-picker-user-section-results = Results
ticket-picker-user-section-staff = Staff
ticket-picker-user-section-all = All users
ticket-picker-user-you-suffix = (you)

# Ticket picker: linked ticket modal (LinkedTicketModal)
ticket-picker-linked-title = Link Ticket
ticket-picker-linked-search-placeholder = Search tickets...
ticket-picker-linked-loading = Loading tickets...
ticket-picker-linked-error = Couldn't load tickets
ticket-picker-linked-try-again = Try Again
ticket-picker-linked-empty-search = No tickets match your search
ticket-picker-linked-empty = No tickets available to link
ticket-picker-linked-col-id = ID
ticket-picker-linked-col-title = Title
ticket-picker-linked-col-status = Status
ticket-picker-linked-col-requester = Requester
ticket-picker-linked-col-updated = Updated
ticket-picker-linked-count = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
}
ticket-picker-linked-cancel = Cancel

# Ticket picker: canned response picker (CannedResponsePicker)
ticket-picker-canned-trigger-aria = Insert canned response
ticket-picker-canned-trigger-title = Insert canned response ({ $shortcut })
ticket-picker-canned-listbox-aria = Canned responses
ticket-picker-canned-loading = Loading…
ticket-picker-canned-empty-title = No canned responses yet.
ticket-picker-canned-empty-hint = Admins can add templates in the admin area.
ticket-picker-canned-load-error = Couldn't load templates

# Ticket picker: device modal (DeviceModal)
ticket-picker-device-title = Add Device
ticket-picker-device-name-label = Name
ticket-picker-device-name-placeholder = Enter device name
ticket-picker-device-hostname-label = Hostname
ticket-picker-device-hostname-placeholder = Enter hostname
ticket-picker-device-serial-label = Serial Number
ticket-picker-device-serial-placeholder = Enter serial number
ticket-picker-device-model-label = Model
ticket-picker-device-model-placeholder = Enter model
ticket-picker-device-warranty-label = Warranty Status
ticket-picker-device-warranty-active = Active
ticket-picker-device-warranty-warning = Warning
ticket-picker-device-warranty-expired = Expired
ticket-picker-device-warranty-unknown = Unknown
ticket-picker-device-cancel = Cancel
ticket-picker-device-add = Add Device

# Document icon selector (DocumentIconSelector)
doc-icon-selector-trigger-aria = Select document icon
doc-icon-selector-search-placeholder = Search icons...
doc-icon-selector-empty = No icons found
doc-icon-selector-footer-hint = Click an icon to pick it
doc-icon-selector-random = Random
doc-icon-selector-scroll-dot-aria = Scroll to section { $index }
doc-icon-selector-category-suggested = Suggested
doc-icon-selector-category-documents = Documents
doc-icon-selector-category-objects = Objects
doc-icon-selector-category-symbols = Symbols
doc-icon-selector-category-nature = Nature
doc-icon-selector-category-animals = Animals
doc-icon-selector-category-people = People
doc-icon-selector-category-travel = Travel
doc-icon-selector-category-food = Food
doc-icon-selector-category-activities = Activities
# Settings: profile (UserProfileCard)
settings-profile-banner-alt = Profile banner
settings-profile-change-photo = Change Photo
settings-profile-name-placeholder = Enter name...
settings-profile-pronouns-label = Pronouns
settings-profile-pronouns-placeholder = Add pronouns (e.g., he/him, she/her, they/them)
settings-profile-save = Save
settings-profile-signature-label = Email signature
settings-profile-signature-hint-prefix = Tacked onto your outbound replies on channel-originated tickets (email). Separator is the standard
settings-profile-signature-hint-suffix = .
settings-profile-signature-placeholder = Tech Name
    IT Support
settings-profile-unknown-user = Unknown User
settings-profile-role-developer = Developer
settings-profile-role-admin = Administrator
settings-profile-role-technician = Technician
settings-profile-role-user = User
settings-profile-error-invalid-file = Invalid file
settings-profile-error-process-image = Couldn't process image
settings-profile-error-user-uuid-missing = User UUID not found
settings-profile-error-not-authenticated = User not authenticated
settings-profile-avatar-upload-success = Profile picture updated
settings-profile-banner-upload-success = Cover image updated
settings-profile-avatar-upload-error = Couldn't upload avatar
settings-profile-banner-upload-error = Couldn't upload banner
settings-profile-avatar-update-error = Couldn't update avatar
settings-profile-banner-update-error = Couldn't update banner
settings-profile-name-update-success = Name updated
settings-profile-name-update-error = Couldn't update name
settings-profile-pronouns-update-success = Pronouns updated
settings-profile-pronouns-update-error = Couldn't update pronouns
settings-profile-signature-update-success = Signature updated
settings-profile-signature-update-error = Couldn't update signature

# Settings: notifications (NotificationSettings)
settings-notifications-category-ticket-label = Tickets
settings-notifications-category-ticket-description = Notifications about ticket assignments and status changes
settings-notifications-category-comment-label = Comments
settings-notifications-category-comment-description = Notifications when someone comments on your tickets
settings-notifications-category-mention-label = Mentions
settings-notifications-category-mention-description = Notifications when someone mentions you
settings-notifications-category-documentation-label = Documentation
settings-notifications-category-documentation-description = Notifications about documentation page updates
settings-notifications-channel-in-app-name = In-App
settings-notifications-channel-in-app-description = Toast notifications while using the app
settings-notifications-channel-email-name = Email
settings-notifications-channel-email-description = Email notifications (rate limited)
settings-notifications-type-ticket-assigned-name = Ticket Assigned
settings-notifications-type-ticket-assigned-description = When you're assigned to a ticket
settings-notifications-type-ticket-status-changed-name = Status Changed
settings-notifications-type-ticket-status-changed-description = When a ticket you're involved with changes status
settings-notifications-type-comment-added-name = New Comment
settings-notifications-type-comment-added-description = When someone comments on your ticket
settings-notifications-type-mentioned-name = Mentioned
settings-notifications-type-mentioned-description = When someone mentions you in a comment
settings-notifications-type-ticket-created-requester-name = Ticket Created
settings-notifications-type-ticket-created-requester-description = When a ticket is created on your behalf
settings-notifications-type-doc-page-updated-name = Page Updated
settings-notifications-type-doc-page-updated-description = When a documentation page you subscribe to is modified
settings-notifications-browser-banner-title = Enable Browser Notifications
settings-notifications-browser-banner-description = Allow browser notifications to receive alerts even when the app isn't in focus.
settings-notifications-browser-banner-enable = Enable Notifications
settings-notifications-browser-enabled-success = Browser notifications enabled
settings-notifications-browser-denied-error = Browser notification permission denied
settings-notifications-quick-settings-title = Quick Settings
settings-notifications-channel-toggle-all-label = All { $channel } Notifications
settings-notifications-column-header = Notification
settings-notifications-load-error = Couldn't load notification preferences
settings-notifications-preference-update-success = Preference updated
settings-notifications-preference-update-error = Couldn't update preference
settings-notifications-channel-bulk-success = All { $channel } notifications { $state ->
    [enabled] enabled
   *[disabled] disabled
}
settings-notifications-info-footer = Email notifications are rate limited to keep your inbox sane. You'll get at most one email per ticket every 5 minutes.

# Settings: passkeys (PasskeySettings)
settings-passkey-section-title = Passkeys
settings-passkey-empty-title = No passkeys registered
settings-passkey-empty-admin-description = This user hasn't registered any passkeys.
settings-passkey-empty-self-description = Sign in with biometrics or a security key instead of a password
settings-passkey-add-button = Add Passkey
settings-passkey-add-another-button = Add Another Passkey
settings-passkey-synced-badge = Synced
settings-passkey-last-used = Last used { $date }
settings-passkey-never-used = Never used
settings-passkey-rename-tooltip = Rename passkey
settings-passkey-delete-tooltip = Delete passkey
settings-passkey-admin-info = Passkey registration needs the account owner's biometrics or security key.
settings-passkey-unsupported-title = Browser Not Supported
settings-passkey-unsupported-description = Your browser doesn't support passkeys (WebAuthn). Please use a modern browser like Chrome, Safari, Firefox, or Edge.
settings-passkey-admin-load-error = Couldn't load passkeys for this user
settings-passkey-admin-delete-success = Passkey has been deleted
settings-passkey-admin-delete-error = Couldn't delete passkey
settings-passkey-add-modal-title = Add Passkey
settings-passkey-add-modal-description = Give your passkey a name so you can spot it later. Your device will prompt you to create the passkey.
settings-passkey-add-modal-name-label = Passkey Name (optional)
settings-passkey-add-modal-name-placeholder = e.g., MacBook Pro, iPhone
settings-passkey-modal-cancel = Cancel
settings-passkey-add-modal-create = Create Passkey
settings-passkey-add-modal-creating = Creating...
settings-passkey-rename-modal-title = Rename Passkey
settings-passkey-rename-modal-name-label = Passkey Name
settings-passkey-rename-modal-placeholder = Enter new name
settings-passkey-rename-modal-save = Save
settings-passkey-delete-modal-title = Delete Passkey
settings-passkey-delete-modal-confirm-prefix = Sure you want to delete
settings-passkey-delete-modal-confirm-suffix = ? You won't be able to use this passkey to sign in.
settings-passkey-delete-modal-password-label = Enter your password to confirm
settings-passkey-delete-modal-password-placeholder = Your password
settings-passkey-delete-modal-confirm = Delete Passkey
settings-passkey-admin-delete-modal-confirm-prefix = Sure you want to delete
settings-passkey-admin-delete-modal-confirm-suffix = ? This user won't be able to sign in with this passkey.
settings-passkey-admin-delete-modal-deleting = Deleting...

# Auth: passkey setup (PasskeySetup)
auth-passkey-setup-unsupported-title = Passkeys Not Available
auth-passkey-setup-unsupported-insecure = Passkeys need a secure connection (HTTPS). You're currently on an insecure connection.
auth-passkey-setup-unsupported-browser = Your browser doesn't support passkeys. Please use a modern browser like Chrome, Safari, Firefox, or Edge, or pick the authenticator app option instead.
auth-passkey-setup-heading = Set Up Passkey
auth-passkey-setup-description = Sign in securely using Face ID, Touch ID, Windows Hello, or a security key.
auth-passkey-setup-name-label = Passkey Name
auth-passkey-setup-name-placeholder = e.g., MacBook Pro, iPhone
auth-passkey-setup-name-hint = A name to identify this passkey later
auth-passkey-setup-create-button = Create Passkey
auth-passkey-setup-creating-button = Creating Passkey...
auth-passkey-setup-device-iphone = iPhone
auth-passkey-setup-device-ipad = iPad
auth-passkey-setup-device-mac = Mac
auth-passkey-setup-device-windows = Windows PC
auth-passkey-setup-device-android = Android Device
auth-passkey-setup-device-linux = Linux PC
auth-passkey-setup-device-generic = This Device
auth-passkey-setup-error-session-expired = Session expired. Please log in again.
auth-passkey-setup-error-cancelled = Registration was cancelled or not allowed
auth-passkey-setup-error-already-registered = This passkey is already registered
auth-passkey-setup-error-cancelled-generic = Registration was cancelled
auth-passkey-setup-error-generic = Couldn't register passkey
auth-passkey-setup-success-message = Passkey created
auth-passkey-setup-backup-codes-title = Save Your Recovery Codes
auth-passkey-setup-backup-codes-description = If you lose access to your passkey, you can use one of these codes to sign in. Each code can only be used once.
auth-passkey-setup-backup-codes-copy = Copy
auth-passkey-setup-backup-codes-copied = Copied!
auth-passkey-setup-backup-codes-download = Download
auth-passkey-setup-backup-codes-acknowledge = I've saved my recovery codes
auth-passkey-setup-backup-file-title = Nosdesk Recovery Codes
auth-passkey-setup-backup-file-intro = Save these codes somewhere safe. Each code can only be used once.
auth-passkey-setup-success-heading = Passkey Created!
auth-passkey-setup-success-description = Your passkey "{ $name }" is ready to use.
auth-passkey-setup-success-protected-title = Your account is protected
auth-passkey-setup-success-protected-description = Next time you sign in, just use your fingerprint, face, or security key instead of a password.
auth-passkey-setup-success-cta = Start Using Nosdesk!

# Settings: emails (UserEmailsCard)
settings-emails-section-title = Email Addresses
settings-emails-add-button = Add Email
settings-emails-add-form-title = Add New Email Address
settings-emails-add-placeholder = email@example.com
settings-emails-add-submit = Add
settings-emails-add-submitting = Adding...
settings-emails-add-cancel = Cancel
settings-emails-empty = No email addresses found
settings-emails-primary-badge = Primary
settings-emails-verified-badge = Verified
settings-emails-unverified-badge = Unverified
settings-emails-type-personal = personal
settings-emails-set-primary = Set as Primary
settings-emails-remove = Remove
settings-emails-confirm-title = Remove email address?
settings-emails-confirm-message = { $email } won't be associated with this account anymore.
settings-emails-confirm-label = Remove
settings-emails-error-required = Email address is required
settings-emails-error-invalid-format = Invalid email format
settings-emails-add-success = Email address added
settings-emails-add-error = Couldn't add email address
settings-emails-set-primary-success = Set { $email } as primary email
settings-emails-set-primary-error = Couldn't set email as primary
settings-emails-delete-success = Email address removed
settings-emails-delete-error = Couldn't delete email address
# Docs: article card (ArticleCard)
docs-article-card-updated = Updated { $date }
docs-article-card-edit = Edit article

# Docs: collection manager modal (CollectionManager)
docs-collection-manager-title = Manage collections
docs-collection-manager-empty = No collections available.
docs-collection-manager-pages = { $count ->
    [one] { $count } page
   *[other] { $count } pages
}
docs-collection-manager-system-badge = System
docs-collection-manager-cancel = Cancel
docs-collection-manager-save = Save
docs-collection-manager-saving = Saving...

# Docs: collection browser (CollectionBrowser)
docs-collection-browser-heading = Collections
docs-collection-browser-new = New
docs-collection-browser-name-placeholder = Collection name...
docs-collection-browser-cancel = Cancel
docs-collection-browser-create = Create
docs-collection-browser-loading-label = Loading collections
docs-collection-browser-pages = { $count ->
    [one] { $count } page
   *[other] { $count } pages
}
docs-collection-browser-system-badge = System
docs-collection-browser-restricted-badge = Restricted
docs-collection-browser-empty = No collections yet.

# Docs: collection tree item (CollectionTreeItem)
docs-collection-tree-item-untitled = Untitled
docs-collection-tree-item-draft = Draft
docs-collection-tree-item-override-title = Custom permissions

# Docs: collection tree list (CollectionTreeList)
docs-collection-tree-list-empty = No pages in this collection yet.

# Docs: collection visibility modal (CollectionVisibilityModal)
docs-collection-visibility-title = Collection access
docs-collection-visibility-description = Pick which groups and users can get to this collection. Leave it empty and the collection is public (everyone can see it).
docs-collection-visibility-public = Public, visible to all users
docs-collection-visibility-picker-placeholder = Search users and groups...
docs-collection-visibility-cancel = Cancel
docs-collection-visibility-save = Save
docs-collection-visibility-saving = Saving...

# Docs: document actions menu (DocumentActionsMenu)
docs-actions-menu-subscribe = Subscribe
docs-actions-menu-unsubscribe = Unsubscribe
docs-actions-menu-insights = Insights
docs-actions-menu-history = Revision history
docs-actions-menu-print = Print
docs-actions-menu-duplicate = Duplicate
docs-actions-menu-export = Download Markdown
docs-actions-menu-move = Move to...
docs-actions-menu-collections = Collections
docs-actions-menu-archive = Archive
docs-actions-menu-unarchive = Unarchive
docs-actions-menu-permissions = Permissions
docs-actions-menu-publish = Publish
docs-actions-menu-unpublish = Unpublish
docs-actions-menu-trash = Move to bin
docs-actions-menu-trash-confirm = Sure? Send to bin?
docs-actions-menu-trigger = Page actions

# Docs: breadcrumb (DocumentationBreadcrumb)
docs-breadcrumb-root = Documentation
docs-breadcrumb-aria = Breadcrumb

# Docs: card (DocumentationCard)
docs-card-empty-content = No content yet
docs-card-children-more = +{ $count } more
docs-card-relative-unknown = Unknown
docs-card-relative-today = Today
docs-card-relative-yesterday = Yesterday
docs-card-relative-days = { $count }d ago
docs-card-relative-weeks = { $count }w ago
docs-card-freshness-fresh = Updated recently
docs-card-freshness-recent = Updated this week
docs-card-freshness-stale = Not updated recently

# Docs: card skeleton (DocumentationCardSkeleton)
docs-card-skeleton-label = Loading pages

# Docs: row skeleton (DocumentationRowSkeleton)
docs-row-skeleton-label = Loading pages

# Docs: nav (DocumentationNav)
docs-nav-starred = Starred
docs-nav-empty = No documents yet
docs-nav-sort-manual = Manual
docs-nav-sort-alpha = Alphabetical
docs-nav-sort-recent = Recently updated
docs-nav-untitled = Untitled
docs-nav-duplicate-suffix = { $title } (copy)
docs-nav-confirm-delete-collection-title = Delete { $name }?
docs-nav-confirm-delete-collection-fallback = Delete collection?
docs-nav-confirm-delete-collection-message = Pages in this collection get sent to the bin. You can restore them from there.
docs-nav-confirm-delete = Delete
docs-nav-menu-open-new-tab = Open in new tab
docs-nav-menu-copy-link = Copy link
docs-nav-menu-copy-md = Copy as Markdown
docs-nav-menu-copy-text = Copy as plain text
docs-nav-menu-add-child = Add child page
docs-nav-menu-star = Star
docs-nav-menu-unstar = Remove star
docs-nav-menu-subscribe = Subscribe
docs-nav-menu-duplicate = Duplicate
docs-nav-menu-move = Move to...
docs-nav-menu-history = Revision history
docs-nav-menu-insights = Insights
docs-nav-menu-export-md = Download Markdown
docs-nav-menu-print = Print
docs-nav-menu-permissions = Permissions
docs-nav-menu-archive = Archive
docs-nav-menu-restore = Restore
docs-nav-menu-trash = Move to bin
docs-nav-col-edit = Edit collection
docs-nav-col-sort-heading = Sort by
docs-nav-col-permissions = Permissions
docs-nav-col-delete = Delete

# Docs: nav row actions (NavRowActions)
docs-nav-row-more = More actions for { $label }
docs-nav-row-add = Add a new page to { $label }

# Docs: nav item (DocumentationNavItem)
docs-nav-item-draft = Draft

# Docs: tree item (DocumentationTreeItem)
docs-tree-item-expand = Expand
docs-tree-item-collapse = Collapse

# Docs: toc item (DocumentationTocItem)
docs-toc-item-untitled = Untitled page

# Docs: insights panel (DocumentInsightsPanel)
docs-insights-title = Insights
docs-insights-source-heading = Source
docs-insights-stats-heading = Stats
docs-insights-contributors-heading = Contributors
docs-insights-created = Created { $relative }
docs-insights-updated = Last updated { $relative }
docs-insights-reading-time = { $minutes ->
    [one] { $minutes } minute read
   *[other] { $minutes } minute read
}
docs-insights-word-count = { $count } words
docs-insights-char-count = { $count } characters
docs-insights-emoji-count = { $count } emoji
docs-insights-contributors-loading = Loading contributors...
docs-insights-contributors-empty = No contributors yet.
docs-insights-contributor-role = Contributor
docs-insights-unknown-user = Unknown user
docs-insights-relative-unknown = unknown
docs-insights-relative-just-now = just now
docs-insights-relative-minutes = { $count } min ago
docs-insights-relative-hours = { $count } hr ago
docs-insights-relative-days = { $count ->
    [one] { $count } day ago
   *[other] { $count } days ago
}
docs-insights-relative-months = { $count ->
    [one] { $count } month ago
   *[other] { $count } months ago
}
docs-insights-relative-years = { $count ->
    [one] { $count } year ago
   *[other] { $count } years ago
}

# Docs: edit collection modal (EditCollectionModal)
docs-edit-collection-title = Edit collection
docs-edit-collection-name = Name
docs-edit-collection-slug = Slug
docs-edit-collection-slug-help = URL fragment for this collection. Lowercase letters, numbers, and dashes only.
docs-edit-collection-icon = Icon
docs-edit-collection-color = Colour
docs-edit-collection-description = Short description
docs-edit-collection-description-placeholder = Optional tagline shown above the collection's overview
docs-edit-collection-description-help = The full overview is edited in place on the collection landing page.
docs-edit-collection-hide-titles-aria = Hide page titles from non-members
docs-edit-collection-hide-titles-label = Hide page titles from non-members
docs-edit-collection-hide-titles-help = Cross-collection wikilinks show "Restricted page" for viewers who can't access it, instead of leaking the title. Recommended for sensitive collections.
docs-edit-collection-name-required = Name's required.
docs-edit-collection-save-error = Couldn't save. Give it another go.
docs-edit-collection-cancel = Cancel
docs-edit-collection-save = Save changes
docs-edit-collection-saving = Saving...

# Docs: move document modal (MoveDocumentModal)
docs-move-title = Move document
docs-move-search-placeholder = Search pages...
docs-move-root-label = Root level (no parent)
docs-move-current-badge = Current
docs-move-empty-search = No matching pages found.
docs-move-empty = No pages available.
docs-move-cancel = Cancel
docs-move-action = Move
docs-move-moving = Moving...

# Docs: page permissions modal (PagePermissionsModal)
docs-page-permissions-title = Page permissions
docs-page-permissions-mode-inherit = Inherit from collections
docs-page-permissions-mode-custom = Custom access
docs-page-permissions-inherit-description = This page inherits visibility from its collections. Anyone who can get to one of its collections can see this page.
docs-page-permissions-no-collections = Not in any collection, visible to everyone.
docs-page-permissions-custom-description = Pick which groups and users can get to this page. This overrides collection-level permissions.
docs-page-permissions-picker-placeholder = Search users and groups...
docs-page-permissions-no-selection-warning = No groups or users selected, no one except admins can see this page.
docs-page-permissions-cancel = Cancel
docs-page-permissions-save = Save
docs-page-permissions-saving = Saving...

# Docs: page ticket links panel (PageTicketLinksPanel)
docs-page-tickets-heading = Linked tickets
docs-page-tickets-add = Link ticket
docs-page-tickets-loading = Loading...
docs-page-tickets-empty = No tickets linked to this page yet.
docs-page-tickets-resolved-heading = Resolved
docs-page-tickets-referenced-heading = Referenced
docs-page-tickets-fallback-title = Ticket #{ $id }
docs-page-tickets-unlink = Unlink ticket #{ $id }

# Docs: author badge (DocumentAuthorBadge)
docs-author-badge-fallback-name = Unknown
docs-author-badge-verifier-fallback = Someone
docs-author-badge-title-verified = Authored by { $author } · verified { $relative }
docs-author-badge-title-basic = Authored by { $author }
docs-author-badge-popover-aria = Document author and verification
docs-author-badge-created = Created
docs-author-badge-author = Author
docs-author-badge-last-edited-by = Last edited by
docs-author-badge-verification = Verification
docs-author-badge-state-verified = Verified
docs-author-badge-state-stale = Stale
docs-author-badge-state-never = Not verified
docs-author-badge-last-verified = Last verified
docs-author-badge-verify-prompt-never = Mark as verified, re-verify every:
docs-author-badge-verify-prompt-again = Re-verify, every:
docs-author-badge-interval-30d = 30d
docs-author-badge-interval-90d = 90d
docs-author-badge-interval-180d = 180d
docs-author-badge-interval-1y = 1y
docs-author-badge-interval-never = Never
docs-author-badge-clear = Clear verification
# Ticket: detail sidebar fields & print header (TicketDetails).
ticket-detail-title-label = Title
ticket-detail-source-label = Source
ticket-detail-source-tooltip = Came in via { $provider }. Replies get relayed back through the thread.
ticket-detail-source-email = Email
ticket-detail-source-slack = Slack
ticket-detail-source-teams = Microsoft Teams
ticket-detail-clear-requester = Clear requester
ticket-detail-add-requester = Add requester
ticket-detail-find-user-placeholder = Find a user...
ticket-detail-assign-to-placeholder = Assign to...
ticket-detail-clear-assignee = Clear assignee
ticket-detail-add-assignee = Add assignee
ticket-detail-claim = Claim
ticket-detail-claim-title = Take this one yourself
ticket-detail-sla-label = SLA
ticket-detail-scheduling-label = Scheduling
ticket-detail-scheduling-none = None
ticket-detail-scheduling-due-date = Due date
ticket-detail-scheduling-due-prefix = Due { $date }
ticket-detail-scheduling-clear-due = Clear due date
ticket-detail-scheduling-recurrence = Recurrence
ticket-detail-recurrence-none = Not recurring
ticket-detail-recurrence-daily = Daily
ticket-detail-recurrence-weekly = Weekly
ticket-detail-recurrence-weekdays = Weekdays
ticket-detail-recurrence-monthly = Monthly
ticket-detail-recurrence-yearly = Yearly
ticket-detail-recurrence-recurring = Recurring
ticket-detail-recurrence-custom-note = Custom RRULE in use ({ $rule }). Edit via the API.
ticket-detail-recurrence-respawn-note = Closing this ticket kicks off the next one.
ticket-detail-category-placeholder = Pick a category...
ticket-detail-cycle-label = Cycle
ticket-detail-cycle-tooltip = Cycle { $name } ({ $state })
ticket-detail-resolution-label = Resolution
ticket-detail-resolution-closed = Closed
ticket-detail-resolution-draft-from-notes = Draft from notes
ticket-detail-resolution-draft-from-notes-title = { $count ->
    [one] Pull { $count } internal note into the resolution draft
   *[other] Pull { $count } internal notes into the resolution draft
  }
ticket-detail-resolution-placeholder-active = What fixed it? Jot it down for the next person.
ticket-detail-resolution-placeholder-draft = Notes on the fix can go here while you're working the ticket.
ticket-detail-audit-created = Created
ticket-detail-audit-modified = Last Modified
ticket-detail-audit-closed = Closed
ticket-detail-print-status = Status
ticket-detail-print-priority = Priority
ticket-detail-print-category = Category
ticket-detail-print-requester = Requester
ticket-detail-print-assignee = Assignee
ticket-detail-print-created = Created
ticket-detail-print-modified = Modified
ticket-detail-print-unassigned = Unassigned
ticket-detail-print-unknown = Unknown
ticket-detail-print-logo-alt = Logo
ticket-detail-print-qr-alt = Ticket QR code
ticket-detail-print-qr-label = Scan to open

# Ticket: comments & attachments composer (CommentsAndAttachments).
ticket-comments-section-title = Comments and Attachments
ticket-comments-drop-files = Drop files here
ticket-comments-internal-banner = Staff only. Not sent through the ticket's channel.
ticket-comments-placeholder-public = Add a new comment...
ticket-comments-placeholder-internal = Note for the team…
ticket-comments-record-voice = Record voice note
ticket-comments-upload-file = Upload file
ticket-comments-visibility-group = Reply visibility
ticket-comments-public-reply = Public reply
ticket-comments-public-reply-title = Goes to the requester through the ticket's channel
ticket-comments-internal-note = Internal note
ticket-comments-internal-note-title = Techs only; never relayed through the ticket's channel
ticket-comments-submit-reply = Add reply
ticket-comments-submit-note = Add note
ticket-comments-voice-note-filename = Voice Note { $date }
ticket-comments-filter-group = Comment visibility filter
ticket-comments-filter-all = All ({ $count })
ticket-comments-filter-public = Public ({ $count })
ticket-comments-filter-internal = Internal ({ $count })
ticket-comments-badge-internal = Internal
ticket-comments-badge-forwarded = Forwarded
ticket-comments-badge-forwarded-title = A tech forwarded this email into the helpdesk
ticket-comments-action-download = Download
ticket-comments-action-delete-comment = Delete comment
ticket-comments-action-delete-voice = Delete voice message
ticket-comments-audio-default = Audio
ticket-comments-audio-voice-message = Voice Message
ticket-comments-print-unknown-author = Unknown
ticket-comments-show-quoted-thread = Show quoted thread
ticket-comments-show-quoted-reply = { $lines ->
    [one] Show quoted reply ({ $lines } line)
   *[other] Show quoted reply ({ $lines } lines)
  }
ticket-comments-show-original = Show original message
ticket-comments-show-original-title = Open the raw RFC-822 source in a new tab

# Ticket: activity timeline (TicketActivity).
ticket-activity-section-title = Activity
ticket-activity-load-error = Couldn't load activity
ticket-activity-load-more-error = Couldn't load more activity
ticket-activity-empty = No activity yet.
ticket-activity-load-more = Load older activity
ticket-activity-loading = Loading…
ticket-activity-actor-someone = Someone
ticket-activity-actor-system = System
ticket-activity-actor-sender = Sender
ticket-activity-actor-email-aria = Email sender
ticket-activity-actor-portal-aria = Public portal submission
ticket-activity-actor-portal-label = the public portal
ticket-activity-channel-email = email
ticket-activity-channel-slack = Slack
ticket-activity-channel-teams = Microsoft Teams
ticket-activity-channel-discord = Discord
ticket-activity-actor-title-subject = { $name } — Subject: { $subject }
ticket-activity-actor-title-named = { $name } <{ $email }>
ticket-activity-actor-title-named-subject = { $name } <{ $email }> — Subject: { $subject }
ticket-activity-to-assignee = to { $name }
ticket-activity-phrase-created = created this ticket
ticket-activity-phrase-opened-via = opened this ticket via { $channel }
ticket-activity-phrase-submitted-via = submitted this ticket via { $channel }
ticket-activity-phrase-deleted = deleted this ticket
ticket-activity-phrase-status-set = set status to { $name }
ticket-activity-phrase-status-changed = changed status
ticket-activity-phrase-reassigned = reassigned this ticket
ticket-activity-phrase-unassigned = unassigned this ticket
ticket-activity-phrase-priority-set = set priority to { $priority }
ticket-activity-phrase-priority-changed = changed priority
ticket-activity-phrase-renamed = renamed the ticket to "{ $title }"
ticket-activity-phrase-renamed-plain = renamed the ticket
ticket-activity-phrase-category-changed = changed the category
ticket-activity-phrase-verification-changed = updated verification state
ticket-activity-phrase-tags-added = { $count ->
    [one] added a tag
   *[other] added { $count } tags
  }
ticket-activity-phrase-tags-removed = { $count ->
    [one] removed a tag
   *[other] removed { $count } tags
  }
ticket-activity-phrase-tags-updated = updated the tags
ticket-activity-phrase-resolution-changed = updated the resolution notes
ticket-activity-phrase-watcher-self-start = started watching this ticket
ticket-activity-phrase-watcher-self-auto = started watching (auto-subscribed on first reply)
ticket-activity-phrase-watcher-self-stop = stopped watching this ticket
ticket-activity-phrase-watcher-added-named = added { $name } as a watcher
ticket-activity-phrase-watcher-added = added a watcher
ticket-activity-phrase-watcher-removed-named = removed { $name } as a watcher
ticket-activity-phrase-watcher-removed = removed a watcher
ticket-activity-phrase-updated = updated the ticket
ticket-activity-phrase-internal-note = added an internal note
ticket-activity-phrase-replied-via = replied via { $channel }
ticket-activity-phrase-comment-via = added a comment via { $channel }
ticket-activity-phrase-commented = commented on this ticket
ticket-activity-phrase-comment-deleted = deleted a comment
ticket-activity-phrase-generic = made a change

# Ticket: tag picker sidebar surface (TicketTagsField).
ticket-field-tags-label = Tags
ticket-field-tags-add = Add tag
ticket-field-tags-remove = Remove { $name }
ticket-field-tags-picker-placeholder = Find or create a tag…
ticket-field-tags-loading = Loading…
ticket-field-tags-no-match = No matching tags.
ticket-field-tags-create = Create "{ $name }"
ticket-field-tags-creating = Creating…
ticket-field-tags-done = Done

# Ticket: watchers sidebar surface (TicketWatchersField).
ticket-field-watchers-label = Watchers
ticket-field-watchers-watching = Watching
ticket-field-watchers-watch = Watch
ticket-field-watchers-watch-title = Watch this ticket for updates
ticket-field-watchers-unwatch-title = Stop watching this ticket
ticket-field-watchers-notify-internal = Notify on internal notes
ticket-field-watchers-public-only = Public replies only
ticket-field-watchers-toggle-on = ON
ticket-field-watchers-toggle-off = OFF
ticket-field-watchers-pref-load-error = Couldn't load preference
ticket-field-watchers-pref-save-error = Couldn't save preference
ticket-field-watchers-overflow-title = { $count ->
    [one] { $count } more
   *[other] { $count } more
  }

# Ticket: device chip row (TicketDevicesField).
ticket-field-devices-label = Devices
ticket-field-devices-add = Add device
ticket-field-devices-detach = Detach device
ticket-field-devices-fallback-name = Device #{ $id }
ticket-field-devices-title-with-model = { $hostname } · { $model }

# Ticket: linked tickets chip row (TicketLinkedTicketsField).
ticket-field-linked-tickets-label = Linked Tickets
ticket-field-linked-tickets-add = Link ticket
ticket-field-linked-tickets-drop = Drop to link

# Ticket: projects chip row (TicketProjectsField).
ticket-field-projects-label = Projects
ticket-field-projects-add = Add to project
ticket-field-projects-remove = Remove from project
ticket-field-projects-fallback = Project #{ $id }

# Ticket: linked documentation chip row (TicketLinkedDocs).
ticket-field-docs-label = Documentation
ticket-field-docs-add = Save as doc
ticket-field-docs-resolves-title = { $title } · resolves this ticket

# Ticket: chips/badges.
ticket-chip-remove = Remove { $label }
ticket-chip-sidebar-remove = Remove
ticket-chip-linked-ticket-fallback = Ticket #{ $id }
ticket-chip-linked-ticket-title = #{ $id } · { $title }
ticket-chip-unlink-ticket = Unlink ticket
ticket-chip-gap-flagged = Flagged for documentation
ticket-chip-gap-view-queue = View in queue →
ticket-chip-gap-remove-flag = Remove flag
ticket-chip-preview-priority = Priority
ticket-chip-preview-created = Created
ticket-chip-preview-requester = Requester
ticket-chip-preview-assignee = Assignee
ticket-chip-preview-unassigned = Unassigned
ticket-chip-preview-unlink = Unlink ticket
ticket-chip-device-warranty-active = Active
ticket-chip-device-warranty-warning = Warning
ticket-chip-device-warranty-expired = Expired
ticket-chip-device-remove = Remove device
ticket-chip-device-view-title = View device
ticket-chip-device-unnamed = Unnamed Device
ticket-chip-device-field-serial = Serial
ticket-chip-device-field-model = Model
ticket-chip-device-field-manufacturer = Manufacturer
ticket-chip-device-field-hostname = Hostname
ticket-chip-device-value-na = N/A
ticket-chip-device-value-unknown = Unknown
ticket-chip-device-copy-tooltip = Click to copy
ticket-chip-device-copied = Copied!

# Ticket: status/priority/category picker (CustomDropdown).
ticket-chip-dropdown-select = Select...
ticket-chip-dropdown-status = Select status
ticket-chip-dropdown-priority = Select priority
ticket-chip-dropdown-category = Select category
ticket-chip-dropdown-option = Select option
# Admin: env-config notice (EnvConfigNotice)
admin-env-notice-title = Configuration via Environment Variables
admin-env-notice-prefix = Settings are configured through environment variables in your
admin-env-notice-suffix = file or Docker environment.

# Admin: system info card (SystemInfoCard)
admin-system-info-title = System Info
admin-system-info-version = Version
admin-system-info-environment = Environment
admin-system-info-uptime = Uptime
admin-system-info-update-to = Update to { $version }
admin-system-info-uptime-days = { $count }d
admin-system-info-uptime-hours = { $count }h
admin-system-info-uptime-minutes = { $count }m
admin-system-info-uptime-seconds = { $count }s

# Admin: category editor (CategoryEditPanel)
admin-categories-edit-title-edit = Edit Category
admin-categories-edit-title-create = New Category
admin-categories-edit-delete-tooltip = Delete category
admin-categories-edit-close-tooltip = Close panel
admin-categories-edit-name-label = Name
admin-categories-edit-name-placeholder = Category name
admin-categories-edit-description-label = Description
admin-categories-edit-description-placeholder = Optional description
admin-categories-edit-icon-label = Icon
admin-categories-edit-icon-folder = Folder
admin-categories-edit-icon-tag = Tag
admin-categories-edit-icon-bug = Bug
admin-categories-edit-icon-settings = Settings
admin-categories-edit-icon-idea = Idea
admin-categories-edit-icon-question = Question
admin-categories-edit-icon-alert = Alert
admin-categories-edit-icon-star = Star
admin-categories-edit-color-label = Colour
admin-categories-edit-active-label = Active
admin-categories-edit-visibility-label = Visible to Groups
admin-categories-edit-visibility-hint = (leave empty for public)
admin-categories-edit-visibility-toggle-aria = Toggle visibility for { $name }
admin-categories-edit-member-count = { $count ->
    [one] { $count } member
   *[other] { $count } members
    }
admin-categories-edit-no-groups = No groups yet. Create some first.
admin-categories-edit-cancel = Cancel
admin-categories-edit-save = Save Changes
admin-categories-edit-create = Create Category

# Admin: group configuration panel (GroupConfigurationPanel)
admin-groups-config-subtitle = Group Configuration
admin-groups-config-delete-tooltip = Delete group
admin-groups-config-close-tooltip = Close panel
admin-groups-config-source-microsoft = Microsoft Entra ID
admin-groups-config-managed-by = Managed by { $source }
admin-groups-config-last-synced = Last synced { $date }
admin-groups-config-unmanage = Unmanage
admin-groups-config-unmanage-processing = Working...
admin-groups-config-sync-settings = Sync Settings
admin-groups-config-general = General Info
admin-groups-config-name-label = Name
admin-groups-config-name-placeholder = Group name
admin-groups-config-description-label = Description
admin-groups-config-description-placeholder = Optional description
admin-groups-config-color-label = Colour
admin-groups-config-save-changes = Save Changes
admin-groups-config-members = Members
admin-groups-config-no-members = No members
admin-groups-config-devices = Devices
admin-groups-config-device-sn = SN: { $sn }
admin-groups-config-no-devices = No devices
admin-groups-config-included-in = Included In
admin-groups-config-included-groups = Included Groups
admin-groups-config-includes-hint = Members of included groups count as members of this group for visibility, access, and assignment.
admin-groups-config-source-direct = Direct
admin-groups-config-source-via = via
admin-groups-config-source-also-via = also via
admin-groups-config-section-assigned = Assigned
admin-groups-config-section-included-via = Included via Groups
admin-groups-config-section-not-assigned = Not Assigned
admin-groups-config-search-users = Search users...
admin-groups-config-search-devices = Search devices by name, hostname, serial...
admin-groups-config-search-groups = Search groups...
admin-groups-config-no-users-found = No users found
admin-groups-config-no-devices-found = No devices found
admin-groups-config-no-groups-found = No groups found
admin-groups-config-synced-badge = Synced
admin-groups-config-synced-intune-tooltip = Synced from Microsoft Intune
admin-groups-config-selected-count = { $count } selected
admin-groups-config-member-count = { $count ->
    [one] { $count } member
   *[other] { $count } members
    }
admin-groups-config-save-members = Save Members
admin-groups-config-save-devices = Save Devices
admin-groups-config-save-includes = Save Included Groups
admin-groups-config-not-found = Group not found
admin-groups-config-cancel = Cancel
admin-groups-config-delete-title = Delete Group
admin-groups-config-delete-confirm = Delete Group
admin-groups-config-delete-prompt-prefix = Sure you want to delete the group
admin-groups-config-delete-prompt-suffix = ? This drops every member link but won't delete the users.
admin-groups-config-unmanage-title = Unmanage group?
admin-groups-config-unmanage-title-named = Unmanage { $name }?
admin-groups-config-unmanage-message = The group won't sync with Microsoft Entra ID anymore. Manual edits become OK, but the sync history sticks around.
admin-groups-config-error-invalid-id = Invalid group ID
admin-groups-config-error-load = Couldn't load group details
admin-groups-config-error-name-required = Group name is required
admin-groups-config-error-save = Couldn't save group
admin-groups-config-error-members = Couldn't update members
admin-groups-config-error-devices = Couldn't update devices
admin-groups-config-error-includes = Couldn't update included groups
admin-groups-config-error-delete = Couldn't delete group
admin-groups-config-error-unmanage = Couldn't unmanage group
admin-groups-config-success-updated = Group updated
admin-groups-config-success-members = Members updated
admin-groups-config-success-devices = Devices updated
admin-groups-config-success-includes = Included groups updated
admin-groups-config-success-unmanage = Group is now locally managed

# Tickets table chrome (TicketsTable, TicketRow, TicketPreviewPane)
views-tickets-table-select-all-aria = Select all visible tickets
views-tickets-table-resize-handle-tooltip = Drag to resize · double-click to fit
views-ticket-row-select-aria = Select ticket #{ $id }
views-ticket-row-recurring-tooltip = Recurring ticket
views-ticket-row-sla-badge = SLA
views-ticket-row-sla-breached-tooltip = SLA breached
views-ticket-row-sla-breached = Breached
views-ticket-row-sla-paused = Paused
views-ticket-row-sla-on-track = On track
views-ticket-row-cycle-tooltip = Part of a cycle
views-ticket-row-cycle-label = cycle #{ $id }
views-ticket-row-no-due-date = No due date
views-ticket-row-kb-badge = KB
views-ticket-row-kb-gap-tooltip = { $signal } knowledge gap signal
views-ticket-row-devices-count = { $count ->
    [one] { $count } device
   *[other] { $count } devices
    }

# Ticket preview pane (TicketPreviewPane)
views-ticket-preview-aria = Ticket preview
views-ticket-preview-empty-title = No ticket selected
views-ticket-preview-empty-prefix = Click any row, or scrub with
views-ticket-preview-empty-suffix = to preview.
views-ticket-preview-open = Open
views-ticket-preview-close-tooltip = Close preview (Esc)
views-ticket-preview-close-aria = Close preview
views-ticket-preview-kb-gap = KB gap
views-ticket-preview-recurring = Recurring
views-ticket-preview-properties = Properties
views-ticket-preview-assignee = Assignee
views-ticket-preview-requester = Requester
views-ticket-preview-due-date = Due date
views-ticket-preview-not-set = Not set
views-ticket-preview-cycle = Cycle
views-ticket-preview-cycle-label = Cycle #{ $id }
views-ticket-preview-category = Category
views-ticket-preview-sla = SLA
views-ticket-preview-activity = Activity
views-ticket-preview-last-activity = Last activity
views-ticket-preview-created = Created
views-ticket-preview-affected-devices = Affected devices
views-ticket-preview-more-devices = { $count ->
    [one] +{ $count } more
   *[other] +{ $count } more
    }
views-ticket-preview-view-full = View description, comments, and devices

# Ticket heatmap (TicketHeatmap)
ticket-heatmap-title-closed = Closed Tickets
ticket-heatmap-title-activity = Ticket Activity
ticket-heatmap-error-load = Couldn't load ticket data. Give it another shot.
ticket-heatmap-tooltip-empty = No tickets
ticket-heatmap-tooltip-count = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
    }
ticket-heatmap-day-sun = Sun
ticket-heatmap-day-mon = Mon
ticket-heatmap-day-tue = Tue
ticket-heatmap-day-wed = Wed
ticket-heatmap-day-thu = Thu
ticket-heatmap-day-fri = Fri
ticket-heatmap-day-sat = Sat
ticket-heatmap-days-with-activity = { $count ->
    [one] { $count } day with activity
   *[other] { $count } days with activity
    }
ticket-heatmap-legend-less = Less
ticket-heatmap-legend-more = More

# Views: add-filter menu (AddFilterMenu)
views-add-filter-trigger = Add filter
views-add-filter-back-tooltip = Back (Backspace)
views-add-filter-search-title-placeholder = Search title…
views-add-filter-no-matches = No values match
views-add-filter-facet-title = Title
views-add-filter-facet-status = Status
views-add-filter-facet-priority = Priority
views-add-filter-facet-assignee = Assignee
views-add-filter-facet-sla = SLA
views-add-filter-facet-cycle = Cycle

# Views: filter value list (FilterValueList)
views-filter-value-search-placeholder = Search…
views-filter-value-no-matches = No matches
views-filter-value-no-options = No options
views-filter-value-clear = Clear

# Views: display menu (DisplayMenu)
views-display-menu-trigger = Display
views-display-menu-trigger-tooltip = Display options
views-display-menu-grouping = Grouping
views-display-menu-density = Density
views-display-menu-density-aria = Row density
views-display-menu-density-compact = Compact
views-display-menu-density-cosy = Cosy
views-display-menu-density-comfortable = Comfortable
views-display-menu-group-none = None
views-display-menu-group-status = Status
views-display-menu-group-priority = Priority
views-display-menu-group-assignee = Assignee
views-display-menu-group-sla = SLA
views-display-menu-group-cycle = Cycle
views-display-menu-properties = Properties
views-display-menu-column-ticket-id = Ticket #
views-display-menu-reset = Reset columns
views-display-menu-reset-tooltip = Restore the view's default column order, widths, and visibility
views-display-menu-save-to-view = Save to view

# Views: tab strips
views-tab-bar-aria = View
views-project-tab-aria = Project view
views-project-tab-board = Board
views-project-tab-gantt = Gantt
views-project-tab-cycles = Cycles

# Views: saved-view editor modal (SavedViewEditorModal)
views-saved-editor-title = Edit view
views-saved-editor-name-label = Name
views-saved-editor-delete = Delete view
views-saved-editor-cancel = Cancel
views-saved-editor-save = Save
views-saved-editor-saving = Saving
views-saved-editor-confirm-title = Delete view?
views-saved-editor-confirm-message = Delete "{ $name }"? Can't undo this one, you'll need to recreate the view if you change your mind.

# Views: filter pill (FilterPill)
views-filter-pill-remove-tooltip = Remove { $label } filter
views-filter-pill-search-title-placeholder = Search title…

# Views: view switcher (ViewSwitcher)
views-view-switcher-placeholder = View
views-view-switcher-edit-view = Edit view…

# Dashboard: user-assigned tickets widget (UserAssignedTickets)
user-assigned-tickets-title-assigned = Assigned Tickets
user-assigned-tickets-title-requested = Requested Tickets
user-assigned-tickets-empty-title-assigned = No assigned tickets
user-assigned-tickets-empty-title-requested = No requested tickets
user-assigned-tickets-empty-current = You're all sorted!
user-assigned-tickets-error-assigned = Couldn't load assigned tickets
user-assigned-tickets-error-requested = Couldn't load requested tickets
user-assigned-tickets-status-active = Active
user-assigned-tickets-status-active-desc = Open + In Progress
user-assigned-tickets-status-open = Open
user-assigned-tickets-status-in-progress = In Progress
user-assigned-tickets-status-closed = Closed
user-assigned-tickets-status-all = All
user-assigned-tickets-status-all-desc = Every status
user-assigned-tickets-status-filter-aria = { $title } status filter
user-assigned-tickets-sort-priority = Priority
user-assigned-tickets-sort-priority-desc = Priority, then recent
user-assigned-tickets-sort-recent = Recent
user-assigned-tickets-sort-recent-desc = Most recently modified
user-assigned-tickets-sort-oldest = Oldest
user-assigned-tickets-sort-oldest-desc = Oldest first, for triage
user-assigned-tickets-filter-high-priority = High priority only
user-assigned-tickets-filter-new-activity = New activity only

# Dashboard: recent tickets widget (RecentTickets)
recent-tickets-empty = No recent tickets
recent-tickets-context-open-new-tab = Open in new tab
recent-tickets-context-copy-link = Copy link
recent-tickets-context-remove = Remove from recent

# Plugin admin: lifecycle state pills (PluginStateBadge)
plugin-state-active = Active
plugin-state-disabled = Disabled
plugin-state-quarantined = Quarantined
plugin-state-uninstalled = Uninstalled

# Plugin admin: trust tier pills (PluginTrustBadge)
plugin-trust-official = Official
plugin-trust-verified = Verified
plugin-trust-community = Community
plugin-trust-local = Local

# Plugin admin: row card (PluginCard)
plugin-card-installed-on = Installed { $date }
plugin-card-permissions = { $count ->
    [one] { $count } permission
   *[other] { $count } permissions
  }
plugin-card-sr-plugin-name = Plugin name
plugin-card-sr-installed = Installed
plugin-card-sr-permission-count = Permission count

# Plugin admin: detail view (PluginDetailView)
plugin-detail-back = Back to plugins
plugin-detail-loading = Loading plugin...
plugin-detail-loading-settings = Loading settings...
plugin-detail-lifecycle-heading = Lifecycle
plugin-detail-settings-heading = Settings
plugin-detail-metadata-heading = Metadata
plugin-detail-metadata-source = Source
plugin-detail-metadata-permissions = Permissions
plugin-detail-metadata-permissions-count = { $count ->
    [one] { $count } declared
   *[other] { $count } declared
  }
plugin-detail-metadata-repository = Repository
plugin-detail-required-aria = required
plugin-detail-secret-configured = Configured
plugin-detail-secret-update = Update
plugin-detail-secret-cancel = Cancel
plugin-detail-secret-placeholder = Enter value
plugin-detail-secret-placeholder-new = Enter new value
plugin-detail-boolean-enabled = Enabled
plugin-detail-action-enable = Enable
plugin-detail-action-disable = Disable
plugin-detail-action-uninstall = Uninstall
plugin-detail-action-discard = Discard
plugin-detail-action-save = Save changes
plugin-detail-action-saving = Saving...
plugin-detail-status-missing-required = { $count ->
    [one] { $count } required field missing
   *[other] { $count } required fields missing
  }
plugin-detail-status-unsaved = { $count ->
    [one] { $count } unsaved change
   *[other] { $count } unsaved changes
  }
plugin-detail-status-all-saved = All changes saved
plugin-detail-toast-saved = { $count ->
    [one] Setting saved
   *[other] Settings saved
  }
plugin-detail-toast-enabled = Plugin enabled
plugin-detail-toast-disabled = Plugin disabled
plugin-detail-error-load = Couldn't load plugin
plugin-detail-error-save = Couldn't save settings. Give it another go.
plugin-detail-error-toggle = Couldn't toggle plugin
plugin-detail-error-uninstall = Couldn't uninstall plugin
plugin-detail-uninstall-title = Uninstall plugin
plugin-detail-uninstall-prompt-prefix = Uninstall
plugin-detail-uninstall-prompt-mid = ? The plugin's
plugin-detail-uninstall-prompt-suffix = policy decides whether its data sticks around or gets cleared.
plugin-detail-uninstall-cancel = Cancel
plugin-detail-uninstall-confirm = Uninstall

# Plugin admin: sideload view (PluginSideloadView)
plugin-sideload-back = Back to plugins
plugin-sideload-title = Sideload signed zip
plugin-sideload-intro-prefix = For plugins that aren't in the registry yet. The bundle has to be signed by a registered publisher or this instance's local signing key; unsigned uploads get knocked back. Chasing an official plugin? Browse the
plugin-sideload-intro-link = registry
plugin-sideload-intro-suffix = first.
plugin-sideload-dropzone-aria = Choose plugin zip file
plugin-sideload-choose-different = Choose a different file
plugin-sideload-drop-here = Drop your plugin zip here
plugin-sideload-or-browse = or click to browse
plugin-sideload-warning-title = Only sideload plugins from sources you trust.
plugin-sideload-warning-prefix = A signature confirms the bundle hasn't been tampered with after signing, but it doesn't vouch for the publisher's intent. An installed plugin runs in the admin UI with access to your session. Stick with the
plugin-sideload-warning-link = registry
plugin-sideload-warning-suffix = for vetted publishers, and have a look through the source of anything you sideload.
plugin-sideload-cancel = Cancel
plugin-sideload-install = Install plugin
plugin-sideload-installing = Installing...
plugin-sideload-error-not-zip = Pick a .zip file
plugin-sideload-error-too-large = File has to be less than 2 MB
plugin-sideload-error-install-failed = Couldn't install plugin
# Guest + Public views (GuestTicketSubmitView, GuestTicketStatusView,
# PublicDocsView, PublicDocView, HelpView, PublicLayout,
# FeatureDisabledNotice). User-facing copy for unauthenticated visitors.

# Shared: feature-disabled notice + public layout
feature-disabled-sign-in = Sign in
public-layout-home-aria = { $appName } home
public-layout-logo-aria = { $appName } Logo
public-layout-nav-aria = Public navigation
public-layout-docs-link = Documentation
public-layout-help-link = Help

# Guest ticket submit
guest-submit-disabled-title = Ticket submission isn't available
guest-submit-disabled-message = Guest ticket submission is currently off. Sign in if you've got an account.
guest-submit-verify-title = Check your inbox
guest-submit-verify-message-prefix = Click the confirmation link we sent to
guest-submit-verify-message-suffix = to release your ticket and set up your portal.
guest-submit-verify-spam-hint = Didn't land? Check spam, then give it another go in a few minutes.
guest-submit-another = Submit another ticket
guest-submit-success-title = Ticket received
guest-submit-success-email-prefix = We've sent a confirmation to
guest-submit-success-email-suffix = with a link to sign in and track progress.
guest-submit-success-no-email = Your ticket's logged. Our team will follow up by email.
guest-submit-success-reference-prefix = Reference number
guest-submit-track-heading = Track without signing in
guest-submit-copied = Copied
guest-submit-copy = Copy
guest-submit-track-hint = Save this link, it's the only way to check the ticket without signing in.
guest-submit-view-status = View ticket status
guest-submit-another-short = Submit another
guest-submit-heading = Submit a ticket
guest-submit-tagline = We'll follow up by email.
guest-submit-honeypot-label = Website
guest-submit-field-name = Your name
guest-submit-field-name-placeholder = Jane Doe
guest-submit-field-email = Email address
guest-submit-field-email-placeholder = you@example.com
guest-submit-field-title = Subject
guest-submit-field-title-placeholder = A short summary of what you need
guest-submit-field-description = Description
guest-submit-field-description-placeholder = Tell us what's going on and how we can help.
guest-submit-description-counter = { $count } / 10000
guest-submit-attachments-label = Attachments
guest-submit-attachments-optional = (optional)
guest-submit-attachments-counter = { $count } / { $max }
guest-submit-attachments-uploading = Uploading...
guest-submit-attachments-pick = Click to attach a file
guest-submit-attachments-hint = Images, PDF, or text. Up to { $size }MB each.
guest-submit-attachments-remove-aria = Remove { $name }
guest-submit-submitting = Submitting...
guest-submit-submit = Submit ticket
guest-submit-have-account = Already have an account?
guest-submit-sign-in = Sign in
guest-submit-error-name = Pop your name in.
guest-submit-error-email = Enter a valid email address.
guest-submit-error-title = Add a subject.
guest-submit-error-description = Tell us what's going on.
guest-submit-error-uploads-pending = Hang on, file uploads are still finishing.
guest-submit-error-rate-limited = Too many submissions from your network. Give it another go later.
guest-submit-error-disabled = Ticket submission's been turned off.
guest-submit-error-account-exists = There's already an account for this email. Sign in to submit a ticket.
guest-submit-error-generic = Couldn't submit the ticket. Give it another go.
guest-submit-error-network = Network hiccup. Give it another go.
guest-submit-attach-error-max = Up to { $max } attachments.
guest-submit-attach-error-too-large = { $name } is over { $size }MB.
guest-submit-attach-error-rate-limited = Too many uploads from your network. Give it another go later.
guest-submit-attach-error-too-large-server = { $name } is too big.
guest-submit-attach-error-disabled = Attachments aren't being accepted right now.
guest-submit-attach-error-generic = Upload didn't work. Give it another go.
guest-submit-attach-error-network = Network hiccup uploading the file.
guest-submit-size-bytes = { $bytes } B
guest-submit-size-kb = { $value } KB
guest-submit-size-mb = { $value } MB

# Guest ticket status
guest-status-loading-aria = Loading ticket
guest-status-disabled-title = Status lookup isn't available
guest-status-disabled-message = Guest ticket status lookup is currently off.
guest-status-ticket-number = Ticket #{ $id }
guest-status-priority = Priority
guest-status-opened = Opened
guest-status-last-updated = Last updated
guest-status-closed = Closed
guest-status-reply-prefix = Need to reply?
guest-status-reply-suffix = to add a comment.
guest-status-not-found-title = Ticket not found
guest-status-not-found-message = The link might've expired or been mistyped.

# Public docs list
public-docs-loading-aria = Loading documentation
public-docs-disabled-title = Documentation isn't available
public-docs-disabled-message = Public documentation is currently off.
public-docs-heading = Documentation
public-docs-tagline = Browse help articles and how-tos.
public-docs-search-placeholder = Search documentation...
public-docs-search-aria = Search documentation
public-docs-no-results = No articles matched your search.
public-docs-empty = No documentation available yet.
public-docs-updated = Updated { $date }

# Public doc detail
public-doc-loading-aria = Loading article
public-doc-back = All docs
public-doc-last-updated = Last updated { $date }
public-doc-rich-text-prefix = This article uses collaborative rich-text editing. A simplified view is shown here, for the full experience with comments and attachments please
public-doc-rich-text-link = sign in
public-doc-rich-text-suffix = .
public-doc-not-found-title = Document not found
public-doc-not-found-message = It might've been moved or set to private.
public-doc-back-to-docs = Back to docs

# Help page
help-disabled-title = Help page isn't available
help-disabled-message = The self-service help page is currently off.
help-heading = How can we help?
help-tagline = Here are a few things you can do without an account.
help-card-submit-title = Submit a ticket
help-card-submit-desc = Report an issue and we'll get back to you by email.
help-card-docs-title = Browse documentation
help-card-docs-desc = Public articles, guides, and how-tos.
help-card-reset-title = Reset your password
help-card-reset-desc = Locked out? Start here.
help-card-signin-title = Sign in
help-card-signin-desc = Already have an account?

# Settings: appearance pane (AppearanceSettings).
settings-appearance-title = Appearance
settings-appearance-theme-heading = Theme
settings-appearance-theme-description = Pick the colour scheme you like
settings-appearance-device-local-label = Device-only theme
settings-appearance-device-local-description = Don't sync theme across devices (e.g. keep E-Paper on your tablet while your laptop stays in dark mode)
settings-appearance-section-automatic = Automatic
settings-appearance-section-light = Light Themes
settings-appearance-section-dark = Dark Themes
settings-appearance-red-horizon-easter-egg = Why would you do this to them 😭
settings-appearance-accessibility-heading = Accessibility
settings-appearance-accessibility-description = Better readability and visual distinction
settings-appearance-colorblind-label = Colour-blind friendly mode
settings-appearance-colorblind-description-monochrome = Always on for monochromatic themes like E-Paper and Red Horizon
settings-appearance-colorblind-description-default = Use distinct shapes for status indicators instead of relying on colour alone
settings-appearance-display-heading = Display
settings-appearance-display-description = Tweak layout preferences
settings-appearance-compact-label = Compact view
settings-appearance-compact-description = Tighten spacing for a denser layout
settings-appearance-theme-changed = Theme changed to { $name }
settings-appearance-theme-changed-device-only = Theme changed to { $name } (device only)
settings-appearance-theme-save-failed = Couldn't save theme preference
settings-appearance-colorblind-toggled = Colour-blind friendly mode { $state ->
    [enabled] on
   *[disabled] off
}
settings-appearance-device-local-toggled = Device-only theme { $state ->
    [enabled] on
   *[disabled] off
}
settings-appearance-compact-toggled = Compact view { $state ->
    [enabled] on
   *[disabled] off
}
settings-appearance-system-theme-name = System

# Settings: ThemeCard.
settings-appearance-card-system-name = System

# Settings: security pane (SecuritySettings).
settings-security-title = Password
settings-security-label-current = Current Password
settings-security-label-new = New Password
settings-security-label-confirm = Confirm New Password
settings-security-placeholder-current = Enter your current password
settings-security-placeholder-new = Enter your new password
settings-security-placeholder-confirm = Confirm your new password
settings-security-placeholder-admin-new = Enter new password
settings-security-placeholder-admin-confirm = Confirm new password
settings-security-hint-length = Password must be at least 8 characters long
settings-security-error-mismatch = Passwords don't match
settings-security-submit-change = Change Password
settings-security-submit-reset = Reset Password
settings-security-error-form-invalid = Please fill in all fields correctly
settings-security-success-changed = Password changed
settings-security-error-change-failed = Couldn't change password. Check your current password.
settings-security-success-reset = Password reset for this user
settings-security-error-reset-failed = Couldn't reset password

# OAuth callback view + card.
auth-callback-loading-default = Finishing sign-in...
auth-callback-loading-processing = Processing authentication...
auth-callback-loading-success = Success! Redirecting...
auth-callback-loading-subtitle = Hang on while we finish authenticating
auth-callback-success-title = Authentication successful
auth-callback-success-subtitle = Redirecting...
auth-callback-technical-details = Technical Details
auth-callback-provider-microsoft = Microsoft
auth-callback-provider-sso = SSO
auth-callback-error-missing-params = Missing required authentication parameters
auth-callback-error-missing-detail = Missing: { $fields }
auth-callback-error-missing-field-code = code
auth-callback-error-missing-field-state = state
auth-callback-error-invalid-response = Invalid response from server
auth-callback-error-no-response = No response received from server
auth-callback-error-unknown = Unknown error
auth-callback-error-generic-message = Something went wrong during authentication
auth-callback-error-status-prefix = Status: { $status }
auth-callback-already-title = Account Already Connected
auth-callback-already-message = This { $provider } account is already linked to another user.
auth-callback-already-suggestion-microsoft = Try a different { $provider } account, or have a chat with your admin.
auth-callback-already-suggestion-generic = Try a different account, or have a chat with your admin.
auth-callback-invalid-title = Authentication Failed
auth-callback-invalid-message = The authentication request was invalid or has expired.
auth-callback-invalid-suggestion-microsoft = Give your { $provider } account another go.
auth-callback-invalid-suggestion-generic = Give signing in another go.
auth-callback-generic-title = Authentication Failed
auth-callback-generic-suggestion = Try again, or contact support if it keeps happening.
auth-callback-action-try-different = Try a Different Account
auth-callback-action-back-settings = Back to Settings
auth-callback-action-return-login = Return to Login
auth-callback-action-try-again = Try Again

# Dashboard widgets
dashboard-widget-shell-action-view-all = View all
dashboard-widget-shell-empty-title-default = Nothing here yet.
dashboard-widget-shell-drag-label = Drag { $title }
dashboard-widget-shell-size-group-label = { $title } size
dashboard-widget-shell-size-option-title = Size { $size } of 3
dashboard-widget-shell-hide-label = Hide { $title }
dashboard-widget-shell-loading-label = Loading { $title }

dashboard-edit-bar-editing = Editing dashboard
dashboard-edit-bar-add-widget = Add widget
dashboard-edit-bar-reset = Reset
dashboard-edit-bar-done = Done
dashboard-edit-bar-reset-confirm-title = Reset dashboard layout?
dashboard-edit-bar-reset-confirm-message = Your custom layout will be swapped back to the default for your role.
dashboard-edit-bar-reset-confirm-label = Reset

dashboard-add-widget-title = Add widget
dashboard-add-widget-all-added = Every widget is already on your dashboard.

dashboard-staff-queue-title = Queue
dashboard-staff-queue-configure-aria = Configure queue metrics
dashboard-staff-queue-configure-title = Configure metrics
dashboard-staff-queue-error = Couldn't load queue metrics
dashboard-staff-queue-metric-unassigned-label = Unassigned
dashboard-staff-queue-metric-unassigned-desc = Open, no assignee
dashboard-staff-queue-metric-all-label = All tickets
dashboard-staff-queue-metric-all-desc = Every status
dashboard-staff-queue-metric-open-label = Open
dashboard-staff-queue-metric-open-desc = Status: open
dashboard-staff-queue-metric-in-progress-label = In progress
dashboard-staff-queue-metric-in-progress-desc = Currently being worked
dashboard-staff-queue-metric-high-priority-label = High priority
dashboard-staff-queue-metric-high-priority-desc = High priority, still open
dashboard-staff-queue-metric-closed-today-label = Closed today
dashboard-staff-queue-metric-closed-today-desc = Closed in the last 24h

dashboard-staff-yours-title = Yours
dashboard-staff-yours-error = Couldn't load counts
dashboard-staff-yours-assigned = Assigned
dashboard-staff-yours-open = Open
dashboard-staff-yours-in-progress = In progress
dashboard-staff-yours-closed = Closed

dashboard-user-summary-title = Summary
dashboard-user-summary-error = Couldn't load summary
dashboard-user-summary-requests = Requests
dashboard-user-summary-open = Open
dashboard-user-summary-in-progress = In progress
dashboard-user-summary-resolved = Resolved

dashboard-queue-metrics-picker-title = Configure queue metrics
dashboard-queue-metrics-picker-hint = Pick up to { $max } metrics to show on the Queue card.
dashboard-queue-metrics-picker-count = ({ $count } / { $max } selected)
dashboard-queue-metrics-picker-toggle-aria = Toggle { $label }
dashboard-queue-metrics-picker-cancel = Cancel
dashboard-queue-metrics-picker-save = Save

dashboard-knowledge-gaps-title = Knowledge gaps
dashboard-knowledge-gaps-title-with-count = Knowledge gaps ({ $count })
dashboard-knowledge-gaps-action = View queue
dashboard-knowledge-gaps-error = Couldn't load gaps
dashboard-knowledge-gaps-empty-title = No open gaps
dashboard-knowledge-gaps-empty-description = Tickets flagged for docs will pop up here.
dashboard-knowledge-gaps-signal-count = { $count -> [one] 1 signal *[other] { $count } signals }
dashboard-knowledge-gaps-impact-tickets = { $count } tickets
dashboard-knowledge-gaps-impact-searches = { $count } searches
dashboard-knowledge-gaps-impact-tooltip-tickets = { $count } tickets showing demand for this doc
dashboard-knowledge-gaps-impact-tooltip-searches = { $count } searches showing demand for this doc

dashboard-channel-health-title = Channel health
dashboard-channel-health-action = Manage
dashboard-channel-health-error = Couldn't load channels
dashboard-channel-health-empty-title = No channels configured
dashboard-channel-health-empty-description = Add an email channel to pull tickets in.
dashboard-channel-health-status-disabled = Disabled
dashboard-channel-health-status-error = Error
dashboard-channel-health-status-healthy = Healthy
dashboard-channel-health-polled = polled { $time }
dashboard-channel-health-never-polled = never polled

dashboard-my-devices-title = My devices
dashboard-my-devices-error = Couldn't load devices
dashboard-my-devices-empty-title = No devices assigned
dashboard-my-devices-empty-description = Devices linked to your account will show up here.
dashboard-my-devices-unknown-model = Unknown model

dashboard-recently-viewed-title = Recently viewed
dashboard-recently-viewed-error = Couldn't load recently viewed
dashboard-recently-viewed-empty-title = Nothing here yet
dashboard-recently-viewed-empty-description = Tickets you open will show up here.

dashboard-starred-docs-title = Starred docs
dashboard-starred-docs-error = Couldn't load starred docs
dashboard-starred-docs-empty-title = No starred pages
dashboard-starred-docs-empty-description = Star a doc to keep it handy.

dashboard-unassigned-queue-title = Unassigned queue
dashboard-unassigned-queue-error = Couldn't load queue
dashboard-unassigned-queue-empty-title = Inbox zero
dashboard-unassigned-queue-empty-description = Nothing waiting in the queue.

# Core UI layer
ui-site-header-untitled-ticket = Untitled ticket
ui-site-header-unknown-device = Unknown device
ui-site-header-ticket-title-placeholder = Ticket title...
ui-site-header-document-title-placeholder = Document title...
ui-site-header-create-aria = Create { $action }
ui-site-header-inbox-tooltip = Inbox
ui-site-header-inbox-aria = Open inbox
ui-user-selection-modal-title = Assign user
ui-user-selection-modal-search-placeholder = Search by name or email...
ui-user-selection-modal-unassign = Unassign
ui-user-selection-modal-error = Couldn't load users
ui-user-selection-modal-empty-no-match = No matches
ui-user-selection-modal-empty-no-users = No users yet
ui-user-selection-modal-role-admin = Admin
ui-user-selection-modal-role-technician = Tech
ui-user-selection-modal-role-user = User
ui-user-card-role-admin = Admin
ui-user-card-role-technician = Tech
ui-user-card-role-user = User
ui-quick-tooltip-unassigned = Unassigned
ui-quick-tooltip-status-label = Status:
ui-quick-tooltip-requester-label = Requester:
ui-quick-tooltip-assignee-label = Assignee:
ui-presence-stack-fallback-name = Someone
ui-presence-stack-aria = { $count -> [one] { $count } person viewing *[other] { $count } people viewing }
ui-presence-stack-overflow-title = { $count -> [one] { $count } more viewing *[other] { $count } more viewing }
ui-status-badge-status-open = open
ui-status-badge-status-in-progress = in progress
ui-status-badge-status-closed = closed
ui-status-badge-priority-low = low
ui-status-badge-priority-medium = medium
ui-status-badge-priority-high = high
ui-status-badge-priority-low-full = low priority
ui-status-badge-priority-medium-full = medium priority
ui-status-badge-priority-high-full = high priority
ui-heatmap-tooltip-more = ...and { $count -> [one] { $count } more *[other] { $count } more }
ui-device-groups-title = Groups
ui-header-title-placeholder = Title...

# Search + ticket remnants
search-global-filter-documentation = Docs
search-global-filter-tickets = Tickets
search-global-filter-devices = Devices
search-global-filter-users = Users
search-global-placeholder = Search tickets, docs, devices, users
search-global-placeholder-filtered = Search { $filter }
search-global-aria-label = Search
search-global-prompt-title = Search your helpdesk
search-global-prompt-subtitle = Find tickets, docs, devices and more
search-global-empty-prefix = Nothing matched
search-global-empty-hint = Try different keywords or check your spelling
search-global-hint-navigate = Move
search-global-hint-open = Open
search-global-hint-close = Close
search-global-results-count = { $count -> [one] { $count } result *[other] { $count } results }
search-global-results-took = { $ms }ms
search-result-item-today = Today
search-result-item-yesterday = Yesterday
search-result-item-days-ago = { $count }d ago
search-result-item-weeks-ago = { $count }w ago
search-result-item-months-ago = { $count }mo ago
search-result-item-years-ago = { $count }y ago
search-result-item-internal-title = Internal note, staff-only
search-result-item-internal-badge = Internal
search-result-group-ticket = Tickets
search-result-group-comment = Comments
search-result-group-documentation = Docs
search-result-group-attachment = Attachments
search-result-group-device = Devices
search-result-group-user = Users
tickets-cycle-burndown-load-error = Couldn't load stats
tickets-cycle-burndown-cat-triage = Triage
tickets-cycle-burndown-cat-backlog = Backlog
tickets-cycle-burndown-cat-active = Active
tickets-cycle-burndown-cat-in-review = In review
tickets-cycle-burndown-cat-done = Done
tickets-cycle-burndown-cat-cancelled = Cancelled
tickets-cycle-burndown-frozen = Frozen
tickets-cycle-burndown-live = Live
tickets-cycle-burndown-loading = Loading...
tickets-cycle-burndown-tickets-done = Tickets done
tickets-cycle-burndown-complete = Complete
tickets-cycle-burndown-days-remaining = { $count -> [one] Day to go *[other] Days to go }
tickets-cycle-burndown-snapshot-frozen = Snapshot frozen { $date }
tickets-collaborative-article-title = Ticket Notes
tickets-collaborative-article-doc-title = Docs: Ticket #{ $id }
tickets-collaborative-article-revision-history = Revision history
tickets-collaborative-article-convert-doc = Turn into a docs page
tickets-collaborative-article-open-full = Open full editor
tickets-project-info-remove = Remove from project
tickets-project-info-description = Description
tickets-project-info-project-id = Project ID
tickets-project-info-status = Status
tickets-project-info-tickets = Tickets
tickets-project-info-print-tickets = { $count -> [one] { $count } ticket *[other] { $count } tickets }
tickets-project-info-status-active = active
tickets-project-info-status-completed = completed
tickets-project-info-status-archived = archived
tickets-email-html-iframe-title = Email body
tickets-email-html-show-less = Show less
tickets-email-html-show-full = Show full email
tickets-email-html-scaled = Scaled to fit ({ $pct }%)

# Header + nav polish
nav-logo-alt = Nosdesk logo
nav-logo-alt-collapsed = Nosdesk
nav-section-recent-tickets = Recent tickets
nav-section-documentation = Documentation
nav-more-heading = More navigation
common-search-placeholder = Search...
header-create-ticket = Create ticket
header-create-project = Create project
header-add-ticket = Add ticket
header-create-user = Create user
header-create-device = Create device
header-create-document = Create document
nav-route-announcement = Navigated to { $title }
common-dropdown-select-placeholder = Pick an option
common-dropdown-empty-message = No matches

# Inbox + notifications polish
inbox-group-today = Today
inbox-group-yesterday = Yesterday
inbox-group-this-week = This week
inbox-group-earlier = Earlier
inbox-mark-mentions-read = Mark mentions as read
inbox-mark-all-read = Mark 'em all as read
inbox-empty-caught-up-title = You're all caught up
inbox-empty-caught-up-subtitle = Nothing unread right now. New notifications will pop up here as they roll in.
inbox-empty-mentions-title = No mentions yet
inbox-empty-mentions-subtitle = When someone @mentions you in a comment, you'll see it here.
inbox-empty-default-title = No notifications yet
inbox-empty-default-subtitle = Updates from tickets, comments, mentions, and docs you follow will land here.
inbox-footer-loading-more = Loading more...
inbox-footer-end-of-feed = That's the lot
notifications-bell-header = Notifications
notifications-bell-open-inbox = Open inbox
notifications-bell-loading = Loading...
notifications-bell-load-more = Load more
notifications-bell-mark-mentions-read = Mark mentions as read
notifications-bell-mark-all-read = Mark 'em all as read
notifications-bell-settings = Settings
notifications-filter-tabs-all = All
notifications-filter-tabs-unread = Unread
notifications-filter-tabs-mentions = Mentions
notifications-toast-new-with-title = New notification: { $title } ({ $seq })
notifications-toast-new = New notification ({ $seq })
# Route meta titles
route-title-login = Sign in
route-title-reset-password = Reset password
route-title-mfa-setup = MFA setup required
route-title-accept-invitation = Accept invitation
route-title-onboarding = Setup
route-title-guest-submit-ticket = Submit a ticket
route-title-guest-ticket-status = Ticket status
route-title-documentation = Documentation
route-title-help = Help
route-title-dashboard = Dashboard
route-title-inbox = Inbox
route-title-tickets = Tickets
route-title-ticket-view = View ticket
route-title-ticket-notes = Ticket #{ $id } notes
route-title-user-profile = User profile
route-title-user-settings = User settings
route-title-user-settings-profile = User profile settings
route-title-user-settings-appearance = User appearance settings
route-title-user-settings-notifications = User notification settings
route-title-user-settings-security = User security settings
route-title-projects = Projects
route-title-cycles = Cycles
route-title-cycle-detail = Cycle
route-title-project-gantt = Gantt
route-title-assets = Assets
route-title-project-detail = Project details
route-title-error = Error
route-title-users = Users
route-title-devices = Devices
route-title-device-create = Create device
route-title-device-view = Device details
route-title-documentation-drafts = Drafts
route-title-collection = Collection
route-title-documentation-archived = Archived
route-title-documentation-trash = Bin
route-title-knowledge-gaps = Knowledge gaps
route-title-profile = Profile
route-title-profile-settings = Settings
route-title-profile-settings-profile = Profile settings
route-title-profile-settings-appearance = Appearance settings
route-title-profile-settings-notifications = Notification settings
route-title-profile-settings-security = Security settings
route-title-administration = Admin
route-title-admin-groups = Groups
route-title-group-configuration = Group setup
route-title-admin-categories = Categories
route-title-admin-assignment-rules = Assignment rules
route-title-admin-workflow = Workflow
route-title-admin-api-tokens = API tokens
route-title-admin-webhooks = Webhooks
route-title-admin-sla = SLA
route-title-admin-plugins = Plugins
route-title-admin-plugin-registry = Plugin registry
route-title-admin-plugin-sideload = Sideload plugin
route-title-admin-plugin-detail = Plugin detail
route-title-admin-auth-providers = Auth providers
route-title-admin-search = Search index management
route-title-admin-system-settings = System settings
route-title-admin-branding = Branding
route-title-admin-audit-log = Audit log
route-title-admin-email-queue = Email queue
route-title-admin-email-suppressions = Email suppressions
route-title-admin-guest-access = Guest access
route-title-admin-email-settings = Email config
route-title-admin-channels-email = Email ingestion
route-title-admin-data-import = Data import
route-title-admin-microsoft-graph = Microsoft Graph connection
route-title-admin-csv-import = CSV import
route-title-admin-backup-restore = Backup and restore
route-title-group-detail = Group details
route-title-authenticating = Authenticating...
route-title-pdf-viewer = PDF viewer

# Loading modals + scattered polish
common-loading-projects = Loading projects...
common-loading-devices = Loading devices...
common-loading-generic = Loading...
common-loading-groups = Loading groups...
common-delete-item-aria = Delete { $name }
admin-branding-aria-logo = Logo
admin-branding-aria-logo-light = Light theme logo
admin-branding-aria-favicon = Favicon

# Store error messages
# en-AU uses the casual "Couldn't X" register rather than "Failed to X".
error-store-workflow-states-load = Couldn't load workflow states.
error-store-public-settings-load = Couldn't load public settings.
error-store-feature-flags-load = Couldn't load feature flags.
error-store-recent-tickets-load = Couldn't fetch recent tickets.
error-store-saved-views-load = Couldn't load saved views.
error-store-saved-view-save = Couldn't save view.
error-store-saved-view-update = Couldn't update view.
error-store-saved-view-delete = Couldn't delete view.
error-store-cycles-load = Couldn't load cycles.
error-store-cycle-create = Couldn't create cycle.
error-store-cycle-update = Couldn't update cycle.
error-store-cycle-complete = Couldn't complete cycle.
error-store-cycle-archive = Couldn't archive cycle.
error-store-auth-profile-load = Couldn't load your profile. Give it another go.
error-store-auth-mfa-setup-start = Couldn't start MFA setup. Give it another go.
error-store-auth-mfa-setup-complete = Couldn't finish MFA setup. Give it another go.

# Q2 polish: helpers + registries + errors
priority-urgent = Urgent
priority-high = High
priority-medium = Medium
priority-low = Low
priority-none = No priority
sla-breached = Breached
sla-at-risk = At risk
sla-on-track = On track
sla-paused = Paused
sla-none = No SLA
status-open = Open
status-in-progress = In progress
status-closed = Closed
status-unknown = Unknown
color-red = Red
color-orange = Orange
color-yellow = Yellow
color-green = Green
color-cyan = Cyan
color-blue = Blue
color-purple = Purple
color-pink = Pink
priority-indicator-low-aria = Low Priority
priority-indicator-medium-aria = Medium Priority
priority-indicator-high-aria = High Priority
priority-indicator-unknown-aria = Unknown Priority
search-entity-type-tickets = Tickets
search-entity-type-comments = Comments
search-entity-type-documentation = Documentation
search-entity-type-attachments = Attachments
search-entity-type-devices = Devices
search-entity-type-users = Users
plugin-permission-ticket-read-label = Read tickets
plugin-permission-ticket-read-description = Read ticket data
plugin-permission-ticket-write-label = Write tickets
plugin-permission-ticket-write-description = Create and update tickets
plugin-permission-ticket-comment-label = Comment on tickets
plugin-permission-ticket-comment-description = Add comments to tickets
plugin-permission-ticket-delete-label = Delete tickets
plugin-permission-ticket-delete-description = Delete tickets
plugin-permission-device-read-label = Read devices
plugin-permission-device-read-description = Read device data
plugin-permission-device-write-label = Write devices
plugin-permission-device-write-description = Create and update devices
plugin-permission-user-read-label = Read users
plugin-permission-user-read-description = Read user profile data
plugin-permission-storage-plugin-label = Plugin storage
plugin-permission-storage-plugin-description = Store plugin-scoped key-value data
plugin-permission-collection-read-label = Read collections
plugin-permission-collection-read-description = Read typed collection rows
plugin-permission-collection-write-label = Write collections
plugin-permission-collection-write-description = Create and update typed collection rows
error-resource-not-found = Couldn't find what you asked for.
error-network = Can't reach the server. Check your connection and try again.
error-session-expired = Your session's expired. Log in again.
error-forbidden = You don't have permission to do that.
plugin-error-load-failed = Couldn't load plugin
plugin-error-pending-review = This plugin is pending review
plugin-error-not-installed = Plugin component isn't installed
plugin-error-component-not-found = Component not found in plugin
plugin-error-timeout = Plugin took too long to load
plugin-error-failed = Plugin failed to load
bulk-action-undo = Undo
bulk-action-undone = Undone
bulk-action-undo-failed = Undo didn't work
passkey-last-used-never = Never

# Dashboard widget registry
dashboard-widget-assigned-tickets-title = Assigned tickets
dashboard-widget-assigned-tickets-description = Your current work queue with status and priority.
dashboard-widget-stats-yours-title = Your counts
dashboard-widget-stats-yours-description = Quick counts of tickets assigned to you by status.
dashboard-widget-stats-queue-title = Queue counts
dashboard-widget-stats-queue-description = Unassigned and total ticket counts across the queue.
dashboard-widget-unassigned-queue-title = Unassigned queue
dashboard-widget-unassigned-queue-description = Oldest open tickets with no assignee. Grab the next one.
dashboard-widget-recently-viewed-title = Recently viewed
dashboard-widget-recently-viewed-description = Tickets you looked at most recently.
dashboard-widget-starred-docs-title = Starred docs
dashboard-widget-starred-docs-description = Documentation pages you have starred for quick access.
dashboard-widget-my-devices-title = My devices
dashboard-widget-my-devices-description = Devices assigned to you as their primary user.
dashboard-widget-channel-health-title = Channel health
dashboard-widget-channel-health-description = Status of inbound email channels, last poll, enabled state, errors.
dashboard-widget-activity-heatmap-title = Activity heatmap
dashboard-widget-activity-heatmap-description = 365-day heatmap of tickets you closed.
dashboard-widget-activity-heatmap-prop-title = Your activity
dashboard-widget-requested-tickets-title = Your requests
dashboard-widget-requested-tickets-description = Tickets you have opened with current status.
dashboard-widget-requested-tickets-prop-title = Your requests
dashboard-widget-stats-summary-title = Request summary
dashboard-widget-stats-summary-description = Count of your requests by status.
dashboard-widget-knowledge-gaps-title = Knowledge gaps
dashboard-widget-knowledge-gaps-description = Top docs to write, ranked by ticket evidence.
# Q1 polish: template attributes + static text
tickets-row-new-activity-tooltip = New activity since you last had a look
tickets-row-new-activity-aria = New activity
common-confirm-delete-title = Confirm delete
common-toast-dismiss = Dismiss
common-error-banner-dismiss = Dismiss error
common-route-progress-aria = Loading
common-bulk-actions-aria = Bulk actions
common-loading-more-aria = Loading more
pagination-controls-page = Page
pagination-controls-show = Show
pagination-controls-id-placeholder = ID
device-modal-title = Pick a device
device-modal-search-placeholder = Search devices by name, hostname, serial, maker, or user...
device-modal-owner = Owner
device-modal-unassigned = Unassigned
device-modal-col-device = Device
device-modal-col-status = Status
device-modal-col-serial = Serial
device-modal-col-user = User
project-modal-title = Add to a project
project-modal-search-placeholder = Search projects by name or description...
project-modal-col-name = Project name
project-modal-col-description = Description
project-modal-col-status = Status
project-modal-col-tickets = Tickets
project-modal-col-action = Action
kanban-recurring-tooltip = Recurring ticket
kanban-recurring-aria = Recurring
kanban-sla-aria = SLA status
calendar-today = Today
calendar-anchor-label = Anchor
calendar-anchor-tooltip = Anchor field comes from the saved view; the picker lands in a later update.
calendar-anchor-due-date = Due date
calendar-anchor-created = Created
calendar-anchor-last-activity = Last activity
gantt-today = Today
gantt-title = Gantt
user-cell-missing-tooltip = This user is no longer around
user-cell-unknown = Unknown
user-settings-managing-for = Managing settings for
user-settings-groups-title = Groups
user-settings-role-management-title = Role management
user-settings-account-setup-title = Account setup
user-settings-account-setup-pending = Pending
user-settings-invitation-pending = Invitation pending
user-settings-resend-invitation-title = Resend invitation email
user-settings-danger-zone-title = Danger zone
user-settings-danger-zone-subtitle = Irreversible, destructive actions live here
user-settings-delete-modal-title = Confirm account deletion
user-settings-delete-item-profile = Profile info and settings
user-settings-delete-item-tickets = All tickets created or assigned to this user
user-settings-delete-item-comments = Comments and activity history
user-settings-delete-item-access = Access to all systems and resources
user-settings-password-placeholder = Enter your password
admin-plugins-list-title = Plugins
admin-plugins-list-aria-filter = Filter plugins
admin-plugins-list-search-placeholder = Search plugins
admin-plugins-list-uninstall-title = Uninstall plugin
inbox-title = Inbox
inbox-aria-filter = Filter notifications
inbox-aria-bulk-actions = Bulk actions
inbox-aria-clear-selection = Clear selection
inbox-aria-select-all = Select all notifications
notifications-bell-aria-trigger = Notifications
notifications-bell-aria-filter = Filter notifications
editor-mentions-hint-select = Select
editor-mentions-hint-close = Close
editor-mentions-helper-type = Type
editor-mentions-helper-suffix = to mention someone
