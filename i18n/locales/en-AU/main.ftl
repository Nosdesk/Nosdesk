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
