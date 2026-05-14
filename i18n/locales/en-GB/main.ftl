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
