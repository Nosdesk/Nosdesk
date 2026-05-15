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
