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

# Login + MFA challenge view.
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
forgot-password-error-default = Failed to send reset email. Please try again.
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

dashboard-greeting-morning = Good morning, { $name }.
dashboard-greeting-afternoon = Good afternoon, { $name }.
dashboard-greeting-evening = Good evening, { $name }.
dashboard-greeting-late-night = Hello { $name }, it's getting late.
dashboard-subtitle = Welcome to your { $app } dashboard
dashboard-edit-button = Edit dashboard
dashboard-guest-fallback = Guest

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
settings-locale-fr-FR = French (France)
settings-locale-nl-NL = Dutch (Netherlands)
settings-timezone-browser-detected = Browser-detected ({ $tz })
settings-timezone-use-device = Use device timezone
settings-timezone-search-placeholder = Search city or offset (e.g. London, UTC+0)
settings-timezone-no-matches = No timezones match that search
settings-save = Save
settings-saving = Saving...
settings-localization-saved = Language and timezone preferences saved
settings-localization-save-failed = Failed to save preferences

auto-ack-default-template = Your request (#{ $ticket_id }) has been received and is being reviewed by our support team. To add additional comments, reply to this email.

inbox-time-just-now = Just now
inbox-time-yesterday = Yesterday at { $time }
inbox-time-weekday = { $day } at { $time }

# First-run admin onboarding.
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

# MFA setup wizard.
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

# Password reset.
password-reset-title = Reset Your Password
password-reset-subtitle = Enter your new password below
password-reset-success-title = Password Reset Complete!
password-reset-success-body = Your password has been updated. You can now log in with your new password.
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

# Invitation / guest-ticket accept.
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

# Admin: audit log.
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

# Admin: email suppression list.
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

# Admin: outbound email queue.
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

# Admin: workflow states.
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

# Admin chrome.
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

# Admin: Email Configuration.
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

# Admin: Guest Access.
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
admin-guest-error-load = Failed to load guest settings
admin-guest-error-save = Failed to save guest settings
admin-guest-saved = Guest access settings saved
