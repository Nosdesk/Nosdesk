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
notif-asset-low-stock = [{ $app }] Low stock: { $title }
notif-sla-breached = [{ $app }] SLA breached: { $title }
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
login-title = Welcome back
# Shared brand hero on the login + onboarding pages.
auth-hero-title = The helpdesk you deserve.
auth-hero-subtitle = Open. Real-time. Yours.
auth-hero-pill = Self-hosted
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
empty-asset-search-prompt-title = Search for assets
empty-asset-search-prompt-description = Start typing to find assets by name, serial number, or user
empty-asset-search-title = No assets found
empty-asset-search-description = Try adjusting your search criteria
empty-users-default-title = No users found
empty-users-default-description = Invite users to get started
empty-users-search-title = No users match your search
empty-users-search-description = Try adjusting your search criteria
empty-assets-default-title = No assets found
empty-assets-default-description = Add your first asset to get started
empty-assets-search-title = No assets match your search
empty-assets-search-description = Try adjusting your search or filters
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
nav-projects = Projects
nav-assets = Assets
nav-asset-planner = Asset Planner

# Tab strip across the top of the asset section. Used in place
# of duplicate sidebar entries for inventory list + planner.
asset-tabs-inventory = Inventory
asset-tabs-planner = Planner
nav-users = Users
nav-documentation = Documentation
nav-inbox = Inbox
nav-collapse = Collapse
nav-workspace-switcher-label = Workspace
nav-workspace-switcher-loading = Loading workspaces…
nav-search = Search
nav-more = More
nav-toggle-sidebar = Toggle sidebar
nav-secondary = Secondary navigation
nav-pins-edit = Edit
nav-pins-done = Done
nav-pins-reset = Reset
nav-pins-edit-hint = { $remaining ->
    [one] Tap stars to choose up to { $max } tiles ({ $remaining } slot left)
   *[other] Tap stars to choose up to { $max } tiles ({ $remaining } slots left)
  }
nav-pins-pin = Pin { $name } to the bottom bar
nav-pins-unpin = Unpin { $name } from the bottom bar
user-menu-aria = User menu
user-menu-view-profile = View Profile
user-menu-account = Account
user-menu-administration = Administration
user-menu-report-problem = Report a problem
user-menu-sign-out = Sign out
user-menu-guest-name = Guest

# In-app bug report modal. Workspace-local diagnostics; the data
# never leaves the operator's deployment.
bug-report-modal-title = Report a problem
bug-report-modal-description-label = What happened?
bug-report-modal-description-placeholder = e.g. tried to save a comment and saw a red banner; was on the assignee dropdown just before.
bug-report-modal-description-hint = A sentence or two is plenty.
bug-report-modal-attachments-hint = We attach the current page, build version, and the last few navigations and API calls. Stays inside this workspace.
bug-report-modal-cancel = Cancel
bug-report-modal-submit = Send report
bug-report-success-toast-title = Report sent
bug-report-success-toast-body = Thanks. An admin can find it in this workspace's diagnostics.
bug-report-error-toast-title = Couldn't send report
bug-report-error-toast-body = Try again in a moment.

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
ticket-list-empty-my-active-title = Nothing on your plate.
ticket-list-empty-my-active-description = You have no unresolved tickets assigned to you.
ticket-list-empty-all-tickets-title = No tickets yet.
ticket-list-empty-all-tickets-description = Tickets created in this workspace will appear here.
ticket-list-empty-unassigned-title = Everything has an owner.
ticket-list-empty-unassigned-description = No active tickets are waiting to be assigned.
ticket-list-empty-overdue-title = Nothing overdue.
ticket-list-empty-overdue-description = Every active ticket is still within its due date.
ticket-list-empty-no-in-view-title = No tickets in this view.
ticket-list-empty-no-in-view-description = Adjust the view filter or pick a different view.
ticket-list-bulk-actions-aria = Bulk actions
ticket-list-bulk-status = Status
ticket-list-bulk-priority = Priority
ticket-list-bulk-assign = Assign
ticket-list-bulk-merge = Merge
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

# Sent to a customer when their ticket is merged into another, only if
# the agent ticks "Tell the customer" in the merge dialog.
merge-notification-customer-template = We're combining your request with a related ticket so our team can respond once. We'll continue to update you on this thread.

# Ticket merge dialog + activity surfaces.
ticket-merge-dialog-title = Merge { $count ->
    [one] 1 ticket
   *[other] { $count } tickets
  }
ticket-merge-destination-label = Destination ticket
ticket-merge-reason-label = Reason (optional)
ticket-merge-reason-placeholder = What ties these tickets together?
ticket-merge-notify-customer-label = Tell the customer their ticket was merged
ticket-merge-notify-customer-help = Sends a templated reply on each source ticket's existing channel thread. Off by default.
ticket-merge-submit-button = Merge { $count } tickets
ticket-merge-cancel-button = Cancel
ticket-merge-conflict-toast = Some of these tickets changed since you opened this dialog. Refresh and retry.
ticket-merge-error-toast = Could not merge the tickets. Please try again.
ticket-merge-success-toast = Merged { $count } tickets into #{ $target_id }
ticket-merge-marker-comment-header = Merged { $count } tickets into this one
ticket-merge-banner-merged-into = This ticket was merged into #{ $target_id } by { $actor } on { $when }.
ticket-merge-banner-open-destination = Open destination
ticket-merge-sidebar-merged-in = Merged in
ticket-merge-toast-just-merged = This ticket was just merged into #{ $target_id }.

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
onboarding-getting-started = Getting started
onboarding-token-help-title = Where's my setup token?
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
onboarding-error-token-required = A setup token is required. Check the server startup logs for the setup URL, or paste the token shown there.
onboarding-error-token-expired = The setup token has expired. Restart the backend to generate a fresh one.
onboarding-error-token-mismatch = The setup token is incorrect. Check the token in the server startup logs and try again.
onboarding-error-token-not-present = Setup is closed. No setup token is available on the server.
onboarding-error-validation = Please correct the highlighted fields and try again.
onboarding-error-email-taken = That email address is already in use.
onboarding-error-setup-complete = Setup has already been completed. This page is no longer available.
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
onboarding-migration-body-prefix = No need to create an account first. Restore your existing backup on the host:
onboarding-migration-body-suffix = then refresh and sign in with your existing credentials.
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
password-reset-page-title = Reset Your Password
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
admin-audit-tier-all = All
admin-audit-tier-app = App
admin-audit-tier-auth = Auth
admin-audit-tier-change = Changes
admin-audit-filter-event = Event type
admin-audit-filter-event-placeholder = e.g. auth.
admin-audit-filter-severity = Severity
admin-audit-severity-any = Any
admin-audit-export = Export JSON
admin-audit-exporting = Exporting…
admin-audit-payload = Payload
admin-audit-target = Target
admin-audit-source-ip = Source IP
admin-audit-source-tier1 = App
admin-audit-source-tier2 = Auth
admin-audit-source-tier3 = Change

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
admin-workflow-states-archive-confirm-title = Archive state?
admin-workflow-states-archive-confirm-label = Archive
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
admin-workflow-states-sla-paused = Pauses SLA
admin-workflow-states-sla-running = Runs SLA
admin-workflow-states-sla-paused-title = Tickets in this state pause the SLA clock. Click to let it run.
admin-workflow-states-sla-running-title = Tickets in this state run the SLA clock. Click to pause it.
admin-workflow-states-sla-now-paused-flash = { $name } now pauses the SLA clock
admin-workflow-states-sla-now-running-flash = { $name } now runs the SLA clock

# Admin: asset-kinds registry (AssetKindsView). Admins define the
# kinds of assets they track and the attributes each kind carries.
admin-asset-kinds-title = Asset Kinds
admin-asset-kinds-description = Define the kinds of assets you track. Each kind has a slug (used internally), a label, and an attribute schema in the JSON Schema subset that describes the fields you want to collect when an asset of that kind is created.
admin-asset-kinds-builtin-heading = Built-in kinds
admin-asset-kinds-builtin-description = These kinds ship with Nosdesk. You can edit the label, description, and attribute schema, but the slug stays fixed so existing assets keep resolving.
admin-asset-kinds-builtin-tag = built-in
admin-asset-kinds-custom-heading = Custom kinds
admin-asset-kinds-custom-description = Admin-defined kinds for whatever you need to track (materials, vehicles, licenses, anything). Slug is immutable once created.
admin-asset-kinds-custom-empty = No custom kinds yet. Use the form below to add one.
admin-asset-kinds-create-heading = Create a new kind
admin-asset-kinds-create-button = Create kind
admin-asset-kinds-edit = Edit
admin-asset-kinds-edit-schema = Edit
admin-asset-kinds-delete = Delete
admin-asset-kinds-save = Save
admin-asset-kinds-cancel = Cancel
admin-asset-kinds-field-slug = Slug
admin-asset-kinds-field-slug-placeholder = e.g. office_supply
admin-asset-kinds-field-label = Label
admin-asset-kinds-field-description = Description
admin-asset-kinds-field-icon = Icon name
admin-asset-kinds-field-sort-order = Sort order
admin-asset-kinds-field-category = Category
admin-asset-kinds-field-attribute-schema = Attribute schema (JSON)
admin-asset-kinds-category-it = IT device
admin-asset-kinds-category-logical = Logical (license, subscription)
admin-asset-kinds-category-physical = Physical (vehicle, equipment)
admin-asset-kinds-category-bulk = Bulk (measured by quantity + unit)
admin-asset-kinds-category-generic = Generic
admin-asset-kinds-saved = Saved

# Schema-conflict surface when an attribute_schema change would invalidate existing rows.
admin-asset-kinds-conflict-heading = { $count } existing asset(s) would no longer validate against this schema:
admin-asset-kinds-conflict-help = Fix the listed assets first, or click Force save to apply the schema change anyway. Force-saved assets stay in the database with their old attributes; the asset detail page will flag them.
admin-asset-kinds-force-save = Force save
admin-asset-kinds-created = Kind created
admin-asset-kinds-deleted = { $label } deleted
admin-asset-kinds-error-load = Failed to load asset kinds
admin-asset-kinds-error-save = Failed to save asset kind
admin-asset-kinds-error-create = Failed to create asset kind
admin-asset-kinds-error-delete = Failed to delete asset kind
admin-asset-kinds-error-slug-required = Slug is required
admin-asset-kinds-error-label-required = Label is required
admin-asset-kinds-error-bad-schema-json = Attribute schema is not valid JSON: { $error }
admin-asset-kinds-delete-confirm-title = Delete asset kind?
admin-asset-kinds-delete-confirm = Delete "{ $label }"? Any existing assets with this kind will keep the value, but you will not be able to create new ones until you add the kind back.

# Asset Kinds — additions for the list/editor split (registry
# polish pass). The list view shows builtin + custom groups with
# search + usage-aware delete; create + edit live on a dedicated
# editor route.
admin-asset-kinds-new = New kind
admin-asset-kinds-back-label = Back to asset kinds
admin-asset-kinds-search-placeholder = Search by label, slug, or description...
admin-asset-kinds-loading = Loading asset kinds...
admin-asset-kinds-empty-title = No asset kinds yet
admin-asset-kinds-empty-description = Create your first asset kind to describe what your team tracks.
admin-asset-kinds-no-matches-title = No matching kinds
admin-asset-kinds-no-matches-description = Nothing matches "{ $query }". Try a different word.
admin-asset-kinds-updated = Updated { $when }
admin-asset-kinds-delete-aria = Delete kind { $label }
admin-asset-kinds-delete-confirm-zero = Delete "{ $label }"? No assets currently use this kind.
admin-asset-kinds-delete-confirm-with-count = Delete "{ $label }"? { $count } existing asset(s) reference this kind. They will keep the slug value, but you won't be able to create new ones until the kind is added back.
admin-asset-kinds-builtin-no-delete = Built-in kinds can't be deleted
admin-asset-kinds-create-title = New asset kind
admin-asset-kinds-edit-title = Edit asset kind
admin-asset-kinds-edit-not-found = That asset kind wasn't found. It may have been deleted in another tab.
admin-asset-kinds-prettify = Prettify JSON
admin-asset-kinds-field-slug-hint = Lowercase letters, digits, and underscores. Used internally; cannot be changed once the kind exists.
admin-asset-kinds-field-slug-locked = Slug is locked after creation so existing asset rows keep resolving.
admin-asset-kinds-field-icon-hint = Optional icon name (e.g. "monitor", "phone"). Shown in the asset picker.
admin-asset-kinds-field-attribute-schema-hint = JSON Schema subset. The Builder view is the default; switch to View JSON for hand-edits.
admin-asset-kinds-view-builder = View Builder
admin-asset-kinds-view-json = View JSON

# Asset Kinds — typed attribute builder (AttributeEditor +
# AttributeRow). Replaces the raw JSON textarea for the common
# cases (Text, Number, Date, Boolean, Select, Multi-select,
# Email, URL). Reference attribute types (User, Asset) ship in
# follow-up commits.
asset-kind-attribute-editor-add = Add attribute
asset-kind-attribute-editor-empty-title = No attributes yet
asset-kind-attribute-editor-empty-description = Click "Add attribute" to describe a field the asset form should collect.
asset-kind-attribute-editor-parse-error = Schema could not be parsed: { $error }. Switch to View JSON to fix it directly.
asset-kind-attribute-row-move = Position
asset-kind-attribute-row-move-up = Move up
asset-kind-attribute-row-move-down = Move down
asset-kind-attribute-row-remove = Remove attribute
asset-kind-attribute-row-name = Name
asset-kind-attribute-row-name-placeholder = e.g. serial_number
asset-kind-attribute-row-name-hint = Lowercase letters, digits, and underscores. Used as the JSON key.
asset-kind-attribute-row-name-invalid = Must be lowercase letters, digits, or underscores, and start with a letter.
asset-kind-attribute-row-kind = Type
asset-kind-attribute-row-required = Required
asset-kind-attribute-row-description = Description
asset-kind-attribute-row-description-placeholder = Optional helper text shown under the field
asset-kind-attribute-row-raw-warning = Unrecognised property shape. Edit this attribute via View JSON; the builder preserves it on save.
asset-kind-attribute-row-max-length = Max length
asset-kind-attribute-row-pattern = Pattern (regex)
asset-kind-attribute-row-pattern-hint = Optional. Use POSIX regex; e.g. ^[A-Z0-9-]+$ for uppercase + digits.
asset-kind-attribute-row-minimum = Minimum
asset-kind-attribute-row-maximum = Maximum
asset-kind-attribute-row-enum-values = Allowed values
asset-kind-attribute-row-enum-remove = Remove value { $value }
asset-kind-attribute-row-enum-add-placeholder = Type a value, press Enter
asset-kind-attribute-row-enum-empty = Add at least one allowed value.
asset-kind-attribute-kind-text = Text
asset-kind-attribute-kind-email = Email
asset-kind-attribute-kind-url = URL
asset-kind-attribute-kind-number = Number (integer)
asset-kind-attribute-kind-decimal = Decimal
asset-kind-attribute-kind-boolean = Yes / No
asset-kind-attribute-kind-date = Date
asset-kind-attribute-kind-datetime = Date and time
asset-kind-attribute-kind-select = Single choice
asset-kind-attribute-kind-multi_select = Multiple choice
asset-kind-attribute-kind-user = User reference
asset-kind-attribute-kind-asset = Asset reference
asset-kind-attribute-kind-raw = Custom (read-only)
asset-kind-attribute-user-loading = Loading users...
asset-kind-attribute-user-none = No user selected
asset-kind-attribute-user-load-error = Failed to load users
asset-kind-attribute-row-asset-scope = Scope to asset kind
asset-kind-attribute-row-asset-scope-any = Any kind
asset-kind-attribute-row-asset-scope-hint = Limits the data-entry picker to assets of the chosen kind. Leave as "Any kind" to allow references to any asset.
asset-kind-attribute-asset-loading = Loading assets...
asset-kind-attribute-asset-none = No asset selected
asset-kind-attribute-asset-load-error = Failed to load assets
asset-kind-attribute-asset-empty-for-scope = No assets of kind "{ $kind }" yet.

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
admin-nav-asset-kinds-title = Asset Kinds
admin-nav-asset-kinds-description = Define the kinds of assets you track and the attributes each kind carries
admin-nav-sla-title = SLA
admin-nav-sla-description = Service-level policies and working-hours calendars
admin-nav-canned-responses-title = Canned Responses
admin-nav-canned-responses-description = Reusable reply templates with substituted variables
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
admin-nav-workspaces-title = Workspaces
admin-nav-workspaces-description = Create, archive, and manage tenant workspaces and their members.

# Admin: Workspaces (AdminWorkspacesView). Tenant lifecycle —
# create, rename, archive, restore, hard-delete.
admin-workspaces-title = Workspaces
admin-workspaces-description = Create and manage tenant workspaces. Slugs are permanent identifiers; archive a workspace before permanently deleting it.
admin-workspaces-create = New workspace
admin-workspaces-include-archived = Include archived
admin-workspaces-loading = Loading workspaces…
admin-workspaces-error-load = Failed to load workspaces
admin-workspaces-col-slug = Slug
admin-workspaces-col-name = Name
admin-workspaces-col-plan = Plan
admin-workspaces-col-domain = Custom domain
admin-workspaces-col-status = Status
admin-workspaces-col-members = Members
admin-workspaces-status-active = Active
admin-workspaces-status-archived = Archived
admin-workspaces-domain-none = —
admin-workspaces-members-link = Manage
admin-workspaces-action-rename = Rename
admin-workspaces-archive = Archive
admin-workspaces-restore = Restore
admin-workspaces-delete = Permanently delete
admin-workspaces-delete-not-archived-hint = Archive this workspace before permanent deletion
admin-workspaces-modal-create-title = Create workspace
admin-workspaces-modal-rename-title = Rename workspace
admin-workspaces-field-slug = Slug
admin-workspaces-field-slug-placeholder = e.g. acme-corp
admin-workspaces-field-slug-hint = Lowercase letters, numbers, and hyphens. Cannot be changed later.
admin-workspaces-field-name = Display name
admin-workspaces-field-name-placeholder = Acme Corporation
admin-workspaces-rename-slug-note = Slug { $slug } cannot be changed.
admin-workspaces-rename-submit = Save name
admin-workspaces-cancel = Cancel
admin-workspaces-created-success = Workspace { $slug } created
admin-workspaces-error-create = Failed to create workspace
admin-workspaces-error-archive = Failed to archive workspace
admin-workspaces-error-restore = Failed to restore workspace
admin-workspaces-error-rename = Failed to rename workspace
admin-workspaces-error-delete = Failed to permanently delete workspace
admin-workspaces-error-slug-required = Slug is required
admin-workspaces-error-name-required = Name is required
admin-workspaces-archive-confirm-title = Archive workspace?
admin-workspaces-archive-confirm-message = Archive { $name }? Members lose access until you restore it. You can permanently delete it after archiving.
admin-workspaces-archive-confirm-label = Archive
admin-workspaces-delete-title = Permanently delete workspace?
admin-workspaces-delete-message = Permanently delete { $name } and all of its data? This cannot be undone.
admin-workspaces-delete-confirm = Permanently delete
admin-workspaces-delete-type-label = Type { $slug } to confirm

empty-workspaces-title = No workspaces yet
empty-workspaces-description = Create a workspace to onboard a new tenant.

# Admin: Workspace members (AdminWorkspaceMembersView).
admin-workspace-members-title = Members
admin-workspace-members-back = Back to workspaces
admin-workspace-members-workspace-label = { $name } ({ $slug })
admin-workspace-members-workspace-fallback = Workspace #{ $id }
admin-workspace-members-description = Invite users and manage their roles in this workspace.
admin-workspace-members-loading = Loading members…
admin-workspace-members-error-load = Failed to load members
admin-workspace-members-archived-notice = This workspace is archived. Restore it before adding new members.
admin-workspace-members-invite-heading = Add member
admin-workspace-members-invite-user-label = User
admin-workspace-members-invite-user-placeholder = Search by name or email…
admin-workspace-members-invite-role-label = Role
admin-workspace-members-invite-submit = Add member
admin-workspace-members-col-user = User
admin-workspace-members-col-role = Role
admin-workspace-members-col-invited = Invited
admin-workspace-members-col-accepted = Accepted
admin-workspace-members-accepted-pending = Pending
admin-workspace-members-role-owner = Owner
admin-workspace-members-role-admin = Admin
admin-workspace-members-role-agent = Agent
admin-workspace-members-role-member = Member
admin-workspace-members-remove = Remove member
admin-workspace-members-remove-confirm-title = Remove member?
admin-workspace-members-remove-confirm-message = Remove { $name } from this workspace? They will lose access immediately.
admin-workspace-members-remove-confirm-label = Remove
admin-workspace-members-last-owner-hint = Promote another member to owner before changing or removing the only owner.
admin-workspace-members-error-last-owner = Cannot change or remove the only owner. Promote another member to owner first.
admin-workspace-members-error-archived = Cannot add members to an archived workspace.
admin-workspace-members-error-user-required = Select a user to invite.
admin-workspace-members-already-member = That user is already a member of this workspace.
admin-workspace-members-added-success = { $name } added to the workspace
admin-workspace-members-error-add = Failed to add member
admin-workspace-members-error-role = Failed to update role
admin-workspace-members-error-remove = Failed to remove member

empty-workspace-members-title = No members yet
empty-workspace-members-description = Use the form above to add the first member.

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

# Admin: System Settings. Avatar thumbnail regeneration. Rebuilds
# thumbnails missing on disk or unset in the database (e.g. after a
# restore, which doesn't ship thumbnails). Idempotent and safe to run.
admin-system-thumbnails-title = Profile image thumbnails
admin-system-thumbnails-description = Rebuild avatar thumbnails that are missing or out of date. Restores omit thumbnails to save space, so run this if profile images look broken after a restore. Safe to run anytime; it only regenerates what is missing.
admin-system-thumbnails-action = Regenerate
admin-system-thumbnails-running = Regenerating...
admin-system-thumbnails-success = Thumbnails regenerated
admin-system-thumbnails-failed = Regeneration failed
admin-system-thumbnails-stat-checked = Checked:
admin-system-thumbnails-stat-regenerated = Regenerated:
admin-system-thumbnails-stat-failed = Failed:
admin-system-thumbnails-error-unexpected = An unexpected error occurred while regenerating thumbnails

# Admin: Search Index Management (SearchManagementView).
admin-search-mgmt-title = Search Index Management
admin-search-mgmt-description = Manage the full-text search index for tickets, documentation, assets, and users.
admin-search-mgmt-stats-title = Index Statistics
admin-search-mgmt-refresh = Refresh
admin-search-mgmt-stats-loading = Loading search index stats
admin-search-mgmt-total-documents = Total Documents
admin-search-mgmt-index-size = Index Size
admin-search-mgmt-status = Status
admin-search-mgmt-status-rebuilding = Rebuilding
admin-search-mgmt-status-ready = Ready
admin-search-mgmt-entity-types = Entity Types
admin-search-mgmt-stats-error = Failed to fetch search index statistics
admin-search-mgmt-rebuild-title = Rebuild Search Index
admin-search-mgmt-rebuild-description = Rebuilds the entire search index from the database. Re-indexes all tickets, comments, documentation pages, attachments, assets, and users. Use this if search results are missing or outdated.
admin-search-mgmt-rebuild = Rebuild Index
admin-search-mgmt-rebuilding = Rebuilding...
admin-search-mgmt-rebuild-success = Index Rebuilt Successfully
admin-search-mgmt-rebuild-failed = Rebuild Failed
admin-search-mgmt-rebuild-stat-tickets = Tickets:
admin-search-mgmt-rebuild-stat-comments = Comments:
admin-search-mgmt-rebuild-stat-docs = Docs:
admin-search-mgmt-rebuild-stat-attachments = Attachments:
admin-search-mgmt-rebuild-stat-devices = Assets:
admin-search-mgmt-rebuild-stat-users = Users:
admin-search-mgmt-rebuild-stat-projects = Projects:
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
admin-guest-default-priority-hint = Applied to every guest submission. Agents can re-triage after.
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
admin-data-import-csv-description = Import data from CSV files, including assets, users, and other resources
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

# Admin: Canned Responses (CannedResponsesView). Reusable reply
# templates the composer picker inserts on demand. Workspace-wide
# shared library; admin-only writes. The {{variable}} allow-list
# mirrors `CANNED_RESPONSE_VARIABLES` in the backend; keep both
# sides in sync when extending.
admin-canned-responses-title = Canned Responses
admin-canned-responses-description = Reusable reply templates agents can insert into the ticket composer. Substitute {"{{"}variable{"}}"} tokens at insert time.
admin-canned-responses-loading = Loading templates...
admin-canned-responses-create = New template
admin-canned-responses-create-title = New canned response
admin-canned-responses-edit-title = Edit canned response
admin-canned-responses-create-submit = Create
admin-canned-responses-save = Save changes
admin-canned-responses-cancel = Cancel
admin-canned-responses-search-placeholder = Search by title or body...
admin-canned-responses-search-aria = Search canned responses
admin-canned-responses-column-name = Name
admin-canned-responses-column-updated = Updated
admin-canned-responses-column-inserts = Inserts
admin-canned-responses-column-inserts-title = Times inserted in the last 30 days
admin-canned-responses-delete-title = Delete template
admin-canned-responses-delete-aria = Delete template { $name }
admin-canned-responses-delete-confirm-title = Delete canned response
admin-canned-responses-delete-confirm-message = Permanently delete "{ $name }"? Agents will no longer see it in the composer picker.
admin-canned-responses-delete-confirm-button = Delete
admin-canned-responses-empty-title = No canned responses yet
admin-canned-responses-empty-description = Create your first reply template and agents can insert it from the ticket composer with one click.
admin-canned-responses-no-matches-title = No matching templates
admin-canned-responses-no-matches-description = Nothing matches "{ $query }". Try a different word.
admin-canned-responses-field-title = Title
admin-canned-responses-field-title-placeholder = e.g. Password reset
admin-canned-responses-field-body = Body
admin-canned-responses-field-body-placeholder = Hi {"{{"}customer_name{"}}"}, ...
admin-canned-responses-field-body-hint = Supported variables: { $variables }
admin-canned-responses-warn-unknown-variables = Unknown variables: { $names }. They will appear verbatim in customer replies; correct or remove them.
admin-canned-responses-error-load = Failed to load canned responses
admin-canned-responses-error-save = Failed to save canned response
admin-canned-responses-error-delete = Failed to delete canned response
admin-canned-responses-error-title-required = Title is required
admin-canned-responses-error-body-required = Body is required
admin-canned-responses-error-unknown-variables = Unknown variables: { $names }. Remove or correct them before saving.
admin-canned-responses-success-created = Canned response created
admin-canned-responses-success-updated = Canned response saved
admin-canned-responses-success-deleted = Canned response deleted
admin-canned-responses-browse-starters = Browse templates
admin-canned-responses-editor-insert-label = Insert:
admin-canned-responses-edit-back-label = Back to canned responses
admin-canned-responses-editor-variable-aria = Variable: { $name }
admin-canned-responses-editor-insert-variable-aria = Insert variable { $name }
admin-canned-responses-edit-not-found = That canned response wasn't found. It may have been deleted in another tab.
admin-canned-responses-preview-heading = Preview
admin-canned-responses-preview-empty = Body is empty. Start typing in the editor to see the preview.
admin-canned-responses-preview-hint = Rendered with sample values. Real tickets substitute the values the picker has at insert time.
admin-canned-responses-starters-title = Start from a template
admin-canned-responses-starters-description = Pick a starter as your starting point. You can edit anything before saving.
admin-canned-responses-starters-loading = Loading starters...
admin-canned-responses-starters-error-load = Failed to load starter templates
admin-canned-responses-starters-use = Use this

# Admin: SLA (SlaAdminView). Working calendars and SLA policies
# side by side; both have inline create-forms.
admin-sla-title = SLA
admin-sla-no-calendars-hint = No calendars. Add one below — every SLA policy needs a calendar to compute its targets.
admin-sla-no-policies-hint = No SLA policies. Add one below — without a policy tickets have no SLA pill.
admin-sla-description = Working calendars and SLA policies feed the per-ticket SLA pill. Nosdesk ships with a default Mon–Fri 9–5 UTC calendar and a 4 h response / 24 h resolution policy. Edit them below, or add new entries for specific categories or priorities.
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
admin-sla-col-targets = Targets
admin-sla-col-matches = Matches
admin-sla-matches-none = none
admin-sla-matches-total = { $count } matched
admin-sla-matches-at-risk = { $count } at risk
admin-sla-matches-breached = { $count } breached
admin-sla-matches-at-risk-title = Tickets within 25% of their response target.
admin-sla-matches-breached-title = Tickets past their response target.
admin-sla-col-calendar = Calendar
admin-sla-default-badge = Default
admin-sla-set-default = Set default
admin-sla-delete = Delete
admin-sla-edit = Edit
admin-sla-save = Save
admin-sla-cancel = Cancel
admin-sla-delete-confirm-title = Delete?
admin-sla-calendar-delete-confirm = Delete this calendar? Policies pointing at it will need a new calendar.
admin-sla-policy-delete-confirm = Delete this policy? Tickets that currently match it will lose their SLA pill until another policy matches them. This cannot be undone.
admin-sla-new-calendar-heading = New calendar
admin-sla-new-policy-heading = New policy
admin-sla-new-calendar-button = New calendar
admin-sla-new-policy-button = New policy
admin-sla-new-calendar-title = New working calendar
admin-sla-new-policy-title = New SLA policy
admin-sla-edit-policy-title = Edit SLA policy
admin-sla-error-save = Failed to save
admin-sla-form-conditions-heading = Conditions
admin-sla-form-targets-heading = Targets
admin-sla-field-name = Name
admin-sla-field-tz = Timezone
admin-sla-field-calendar = Calendar
admin-sla-field-response = Response (minutes)
admin-sla-field-resolution = Resolution (minutes)
admin-sla-field-priority = Priority filter
admin-sla-field-category = Category filter
admin-sla-field-assignee-group = Assignee group filter
admin-sla-placeholder-name = EU support hours
admin-sla-placeholder-tz = Select a timezone
admin-sla-tz-search-placeholder = Search timezones...
admin-sla-tz-no-matches = No matching timezones
admin-sla-policy-name-placeholder = Critical incidents
admin-sla-edit-calendar-title = Edit working calendar
admin-sla-field-schedule = Working hours
admin-sla-schedule-day-mon = Mon
admin-sla-schedule-day-tue = Tue
admin-sla-schedule-day-wed = Wed
admin-sla-schedule-day-thu = Thu
admin-sla-schedule-day-fri = Fri
admin-sla-schedule-day-sat = Sat
admin-sla-schedule-day-sun = Sun
admin-sla-schedule-remove-range-aria = Remove this range
admin-sla-schedule-resize-open-aria = Drag to change open time
admin-sla-schedule-resize-close-aria = Drag to change close time
admin-sla-schedule-timeline-hint = Click an empty track to add hours; drag a bar's edges to resize, or click the bar to type a precise time.
admin-sla-schedule-edit-range-aria = Edit time range
admin-sla-field-holidays = Holidays
admin-sla-holidays-empty-hint = No holidays yet. Add a date below to mark it as non-working.
admin-sla-holiday-date = Date
admin-sla-holiday-label = Label
admin-sla-holiday-placeholder = e.g. Bank holiday
admin-sla-holiday-add = Add
admin-sla-holiday-remove-aria = Remove this holiday
admin-sla-holiday-annual = Recurs annually
admin-sla-holiday-annual-hint = Repeat the same MM-DD every year (e.g. Christmas Day).
admin-sla-holiday-annual-badge = Annual
admin-sla-holiday-import-label = Import preset:
admin-sla-holiday-import-placeholder = Choose country...
admin-sla-holiday-import-summary = { $country }: added { $added }, skipped { $skipped } (already present)

# SLA "Why this SLA?" popover, opened by clicking the pill on the
# ticket detail sidebar. Makes the compute-on-read engine's
# reasoning visible in place rather than as a separate report.
sla-explain-aria = SLA explanation
sla-explain-title = Why this SLA?
sla-explain-error = Couldn't load SLA explanation.
sla-explain-no-policy = No SLA policy matched this ticket, so no targets apply.
sla-explain-default-badge = Workspace default
sla-explain-no-filters = Matched as the workspace default (no filters set).
sla-explain-filter-priority = Priority is { $value }
sla-explain-filter-category = Category is { $name }
sla-explain-filter-group = Assignee is in { $name }
sla-explain-calendar-label = Calendar
sla-explain-targets-label = Targets
sla-explain-targets = { $response } response · { $resolution } resolution
sla-explain-state-label = Status
sla-explain-state-running = Clock running ({ $state })
sla-explain-state-paused = Clock paused ({ $state })
sla-explain-fmt-minutes = { $n }m
sla-explain-fmt-hours = { $n }h
sla-explain-fmt-days = { $n }d

ticket-detail-sla-explain-aria = Show why this SLA was picked

# Shared TimePicker + DatePicker primitive aria copy.
time-picker-hours-aria = Hours
time-picker-minutes-aria = Minutes
date-picker-prev-month-aria = Previous month
date-picker-next-month-aria = Next month
admin-sla-priority-any = Any
admin-sla-category-any = Any
admin-sla-assignee-group-any = Any
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
admin-branding-signature-default-label = Default email signature
admin-branding-signature-default-placeholder = Best regards, The Support Team
admin-branding-signature-default-hint = Used for agents who haven't set a personal signature. Leave blank to send replies unsigned.
admin-branding-signature-default-variables-hint = Variables (filled in per reply):
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
admin-channels-email-description = Poll a support mailbox over IMAP and turn inbound messages into tickets. Replies from agents are relayed back through the same thread.
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
admin-channels-email-test-dirty-hint = Save changes first to test against them.
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
admin-channels-email-error-load = Failed to load email channel
admin-channels-email-success-update = Channel updated
admin-channels-email-success-create = Channel created
admin-channels-email-success-password-removed = Password removed
admin-channels-email-success-delete = Channel deleted
admin-channels-email-auto-ack-heading = Auto-acknowledgement
admin-channels-email-auto-ack-subtitle = When a new email opens a ticket, send a brief "we got your message" reply so the customer knows it landed.
admin-channels-email-auto-ack-toggle-label = Send auto-acknowledgement
admin-channels-email-auto-ack-toggle-description = Disable if your team prefers to reply manually within minutes.
admin-channels-email-auto-ack-template-label = Custom template
admin-channels-email-auto-ack-template-placeholder = Hi {"{{"}customer_name{"}}"}, we received your message and will be in touch shortly. (ref #{"{{"}ticket_id{"}}"})
admin-channels-email-auto-ack-template-hint = Leave blank to use the localized default. Plain text only.
admin-channels-email-auto-ack-variables-hint = Variables (filled in per ticket):
admin-channels-email-auto-ack-saving = Saving…
admin-channels-email-auto-ack-save = Save auto-ack
admin-channels-email-auto-ack-success-saved = Auto-acknowledgement updated

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
admin-webhooks-category-assets = Assets
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
admin-webhooks-event-asset-linked = Asset Linked
admin-webhooks-event-asset-unlinked = Asset Unlinked
admin-webhooks-event-asset-updated = Asset Updated
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
user-mgmt-filter-name-label = Name
user-mgmt-filter-role-label = Role
user-mgmt-filter-deleted-label = Deleted
user-mgmt-filter-deleted-on = Show deleted
user-mgmt-grouping-role = Role
user-mgmt-grouping-status = Status
user-mgmt-grouping-status-active = Active
user-mgmt-grouping-status-deleted = Deleted
user-mgmt-grouping-joined = Joined
user-mgmt-grouping-joined-this-month = Last 30 days
user-mgmt-grouping-joined-this-year = This year
user-mgmt-grouping-joined-older = Earlier
user-mgmt-role-admin = Admin
user-mgmt-role-technician = Agent
user-mgmt-role-audit_reviewer = Audit reviewer
user-mgmt-role-user = User
user-mgmt-column-user = User
user-mgmt-column-role = Role
user-mgmt-column-tickets = Tickets
user-mgmt-column-assets = Assets
user-mgmt-column-joined = Joined
user-mgmt-invite-action = Invite User
user-mgmt-mobile-tickets = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
   }
user-mgmt-mobile-assets = { $count ->
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
    [one] Delete one user? They will be restorable for 30 days, then permanently removed.
   *[other] Delete { $count } users? They will be restorable for 30 days, then permanently removed.
}
user-mgmt-bulk-action-error = Failed to perform bulk action. Please try again.
user-mgmt-deleted-off = Show deleted
user-mgmt-deleted-on = Hide deleted
user-mgmt-deleted-badge = Deleted
user-mgmt-deleted-purges-on = Purges on { $date }
user-mgmt-restore = Restore user
user-mgmt-restored = { $name } restored
user-mgmt-restore-error = Failed to restore user.
user-mgmt-purge-now = Permanently delete
user-mgmt-purged = { $name } permanently deleted
user-mgmt-purge-error = Failed to permanently delete user.
user-mgmt-purge-title = Permanently delete user?
user-mgmt-purge-message = Permanently delete { $name }? This skips the 30-day restore window and cannot be undone.
user-mgmt-purge-confirm = Permanently delete
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
user-profile-role-technician = Agent
user-profile-role-audit_reviewer = Audit reviewer
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
user-profile-assets-title = Assets
user-profile-assets-empty = No assets
user-profile-asset-manufacturer-unknown = Unknown
user-profile-asset-last-updated = Last updated { $when }
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
groups-mgmt-sort-assets = Assets
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
group-detail-section-devices = Assets
group-detail-no-members = No members
group-detail-no-devices = No assets
group-detail-unknown-device = Unknown asset
group-detail-not-found = Group not found

# Devices list (DevicesListView): paginated table with warranty
# filter, sortable columns, bulk delete, and mobile row layout.
assets-list-search-placeholder = Search assets...
assets-list-item-label = asset
assets-list-filter-warranty-active = Active
assets-list-filter-warranty-warning = Warning
assets-list-filter-warranty-expired = Expired
assets-list-filter-warranty-unknown = Unknown
assets-list-filter-warranty-all = All Warranties
assets-list-filter-name-label = Name
assets-list-filter-warranty-label = Warranty
assets-list-filter-low-stock-label = Low stock
assets-list-column-device = Asset
assets-list-column-serial = Serial
assets-list-column-hostname = Hostname
assets-list-column-model = Model
assets-list-column-user = User
assets-list-column-warranty = Warranty

assets-list-column-stock = Stock
assets-list-filter-low-stock-all = All stock
assets-list-filter-low-stock-on = Low stock only
assets-list-add-action = Add asset
assets-list-unassigned = Unassigned
assets-list-warranty-unknown = Unknown
assets-list-bulk-delete = Delete
assets-list-bulk-delete-count = Delete { $count }
assets-list-bulk-delete-title = { $count ->
    [one] Delete device?
   *[other] Delete { $count } devices?
}
assets-list-bulk-delete-message = { $count ->
    [one] This will permanently delete one device. This action cannot be undone.
   *[other] This will permanently delete { $count } devices. This action cannot be undone.
}
assets-list-bulk-action-error = Failed to delete assets. Please try again.

# Device detail (DeviceView): per-device page covering name, hostname,
# hardware identifiers, warranty fields, primary user, Microsoft Intune
# integration, and the unmanage / create flows.
asset-detail-back-to-ticket = Back to Ticket #{ $id }
asset-detail-back-to-devices = Go back
asset-detail-readonly = Read-only
asset-detail-delete-item-name = Asset
asset-detail-error-invalid-id = Invalid asset ID
asset-detail-error-load = Failed to load asset details
asset-detail-error-create = Failed to create asset. Please try again.
asset-detail-error-delete = Failed to delete asset. Please try again.
asset-detail-error-unmanage = Failed to unmanage device. Please try again.
asset-detail-section-details = Asset details
asset-detail-section-kind = Asset Kind
asset-detail-field-kind = Kind
asset-detail-field-name = Name
asset-detail-field-name-placeholder-create = Enter asset name
asset-detail-field-name-placeholder-edit = Enter name...
asset-detail-field-hostname = Hostname
asset-detail-field-hostname-placeholder-create = Enter hostname
asset-detail-field-hostname-placeholder-edit = Enter hostname...
asset-detail-field-serial = Serial Number
asset-detail-field-serial-placeholder-create = Enter serial number
asset-detail-field-serial-placeholder-edit = Enter serial number...
asset-detail-field-manufacturer = Manufacturer
asset-detail-field-manufacturer-placeholder-create = e.g., Dell, HP, Apple
asset-detail-field-manufacturer-placeholder-edit = Enter manufacturer...
asset-detail-field-model = Model
asset-detail-field-model-placeholder-create = Enter asset model
asset-detail-field-model-placeholder-edit = Enter model...
asset-detail-field-warranty-status = Warranty Status
asset-detail-field-warranty-start = Warranty Start
asset-detail-field-warranty-end = Warranty End
asset-detail-field-purchase-date = Purchase Date
asset-detail-field-asset-tag = Asset Tag
asset-detail-field-asset-tag-placeholder-create = Enter asset tag
asset-detail-field-asset-tag-placeholder-edit = Enter asset tag...
asset-detail-warranty-active = Active
asset-detail-warranty-warning = Warning
asset-detail-warranty-expired = Expired
asset-detail-warranty-unknown = Unknown
asset-detail-section-primary-user = Primary User
asset-detail-no-user-assigned = No user assigned to this asset
asset-detail-action-assign-user = Assign User
asset-detail-action-change-user = Change User
asset-detail-section-device-information = Device Information
asset-detail-field-device-id = Device ID
asset-detail-field-created = Created
asset-detail-field-last-updated = Last Updated
asset-detail-manually-managed = Manually Managed
asset-detail-manually-managed-description = This device was created and is managed manually in Nosdesk
asset-detail-section-microsoft-integration = Microsoft Integration
asset-detail-field-last-intune-check-in = Last Intune Check-in
asset-detail-action-view-in-intune = View in Intune
asset-detail-action-view-in-entra = View in Entra
asset-detail-action-unmanage = Unmanage from Intune/Entra
asset-detail-action-unmanage-processing = Processing...
asset-detail-action-unmanage-title = Remove from Microsoft Intune/Entra management
asset-detail-unmanage-conversion-note = This will convert the device to manual management
asset-detail-tech-details-show = Show Technical Details
asset-detail-tech-details-hide = Hide Technical Details
asset-detail-field-intune-id = Intune ID
asset-detail-field-entra-id = Entra ID
asset-detail-not-managed-by-intune = This device is not managed by Microsoft Intune
asset-detail-action-cancel = Cancel
asset-detail-action-create = Create asset
asset-detail-action-create-processing = Creating...
asset-detail-not-found = Asset not found
asset-detail-unmanage-modal-title = Unmanage Device
asset-detail-unmanage-heading = Unmanage from Microsoft
asset-detail-unmanage-confirm-body = Are you sure you want to unmanage <strong class="text-primary">{ $name }</strong> from Microsoft Intune/Entra?
asset-detail-unmanage-confirm-note = This will convert the device to manual management. You'll be able to edit all fields, but the device will no longer sync with Microsoft.
asset-detail-unmanage-action-confirm = Unmanage

# Projects list (ProjectsView): workspace-wide grid of projects
# rendered from the sync engine pool, with status pills and a
# short description per card.
projects-list-heading = Projects
projects-list-subheading = Group related tickets and track them together.
projects-list-no-description = No description
projects-empty-title = No projects yet
projects-empty-subtitle = Group related tickets into a project to plan and track them together.
projects-empty-cta = Create your first project
projects-create-title = New project
projects-create-name-label = Name
projects-create-name-placeholder = e.g. Q3 laptop rollout
projects-create-description-label = Description
projects-create-description-placeholder = Optional
projects-create-submit = Create project
projects-create-cancel = Cancel
projects-create-error = Couldn't create the project. Please try again.
projects-filter-status-all = All
projects-list-no-results = No projects match your filters.

# Projects list enrichment: status breakdown, active cycle, sort,
# and desktop column headers.
projects-no-active-cycle = No active cycle
projects-progress-done = { $done }/{ $total } done
projects-status-summary = { $done } done, { $doing } in progress, { $open } open of { $total }
projects-sort-label = Sort
projects-sort-name = Name
projects-sort-recent = Recent
projects-sort-progress = Progress
projects-sort-tickets = Tickets
projects-col-project = Project
projects-col-progress = Progress
projects-col-team = Team
projects-col-cycle = Cycle
projects-col-updated = Updated

# Project detail (ProjectDetailView): per-project kanban board
# with a header, status pill, ticket count, and a Group-by
# control on the kanban toolbar.
project-detail-loading-name = Loading…
project-actions-menu-trigger = Project actions
project-actions-rename = Rename
project-actions-status-active = Active
project-actions-status-completed = Completed
project-actions-status-archived = Archived
project-actions-delete = Delete project
project-delete-confirm-title = Delete project?
project-delete-confirm-message = This permanently deletes "{ $name }" and unlinks its tickets. The tickets themselves are kept. This can't be undone.
project-delete-confirm-button = Delete
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
project-cycles-confirm-complete-title = Complete cycle?
project-cycles-confirm-archive-title = Archive cycle?
project-cycles-confirm-complete = Complete this cycle? The snapshot freezes once you do.
project-cycles-confirm-archive = Archive this cycle?
project-cycles-create-title = New cycle
project-cycles-field-name = Name
project-cycles-field-start = Start
project-cycles-field-end = End
project-cycles-name-placeholder = e.g. Sprint 14
project-cycles-create-submit = Create
project-cycles-velocity-hint = Recent velocity: ~{ $count } tickets per cycle
project-cycles-all-title = All cycles
project-cycles-active-work-title = This cycle's work
project-cycles-ended-warning = This cycle ended on { $date } but hasn't been completed yet.
project-cycles-empty-prefix = No cycles yet. Click
project-cycles-empty-cta = New cycle
project-cycles-empty-suffix = to start an iteration.
project-cycles-state-planned = planned
project-cycles-state-active = active
project-cycles-state-completed = completed
project-cycles-action-promote = Promote
project-cycles-action-complete = Complete
project-cycles-action-archive = Archive

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
asset-planner-empty = No assets match the current filters.
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

# Phase 1 wizard: upload -> review -> done.
csv-import-step-upload = Upload
csv-import-step-review = Review
csv-import-step-done = Done
csv-import-step-upload-heading = Upload your CSV
csv-import-type-label = What are you importing?
csv-import-type-assets = Assets
csv-import-type-users = Users
csv-import-type-tickets = Tickets
csv-import-type-coming-soon = coming soon
csv-import-template-label = Start with our template
csv-import-template-help = The CSV needs to use these exact column headers. Download the empty template, fill it in, then upload it here.
csv-import-template-button = Download template
csv-import-file-label = Choose your file
csv-import-drop-zone-idle = Drop a CSV here, or click to browse
csv-import-drop-here = Release to upload
csv-import-drop-zone-hint = .csv up to 10 MB
csv-import-drop-zone-replace = Click or drop another file to replace
csv-import-error-not-csv = "{ $name }" is not a CSV file
csv-import-action-validate = Validate
csv-import-summary-rows = Rows in file
csv-import-empty-file = This file has only a header row. Fill in some data rows below the header, save the CSV, and upload again.
csv-import-summary-create = Will create
csv-import-summary-update = Will update
csv-import-errors-heading = { $count } row(s) have errors
csv-import-errors-truncated = showing first 100
csv-import-errors-row = Row
csv-import-errors-column = Column
csv-import-errors-message = Message
csv-import-action-discard = Discard
csv-import-action-apply = Apply ({ $count } rows)
csv-import-action-new = New import
csv-import-action-view-assets = View assets
csv-import-action-view-users = View users
csv-import-action-view-tickets = View tickets
csv-import-done-heading = Import complete
csv-import-done-body = { $count } row(s) committed.
csv-import-error-generic = Import failed. Check the file and try again.
csv-import-error-commit-failed = Apply failed; rows were not committed.
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
csv-import-template-devices-name = Assets template
csv-import-template-devices-description = Import assets with hardware details and ownership information
csv-import-template-tickets-name = Tickets Template
csv-import-template-tickets-description = Import support tickets with details and assignees
csv-import-template-download = Download
csv-import-modal-import-title = Import Data from CSV
csv-import-modal-data-type = Data Type
csv-import-modal-type-users = Users
csv-import-modal-type-devices = Assets
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

# Shared common chrome — keys used by primitive components like
# Modal.vue. Kept under the "common-*" namespace so primitives
# don't depend on feature-specific FTL slices.
common-modal-close = Close modal
form-textarea-resize-grip-label = Drag to resize

# Editor: toolbar (CollaborativeEditor)
editor-toolbar-text-style = Text Style
editor-toolbar-bold = Bold
editor-toolbar-italic = Italic
editor-toolbar-bullet-list = Bullet List
editor-toolbar-numbered-list = Numbered List
editor-toolbar-insert = Insert
editor-toolbar-undo = Undo
editor-toolbar-redo = Redo
editor-toolbar-revision-history = Revision History
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
editor-type-menu-code-block = Code Block

# Editor: insert menu (CollaborativeEditor)
editor-insert-menu-bullet-list = Bullet List
editor-insert-menu-numbered-list = Numbered List
editor-insert-menu-blockquote = Blockquote
editor-insert-menu-code-block = Code Block
editor-insert-menu-link = Link
editor-insert-menu-embed-document = Embed Document

# Editor: code block language prompt (CollaborativeEditor)
editor-code-block-language-prompt = Enter language for syntax highlighting (optional):

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
editor-doc-picker-title = Embed Document
editor-doc-picker-close = Close
editor-doc-picker-search-placeholder = Search documents...
editor-doc-picker-empty = No documents found.

# Editor: revision history panel (RevisionHistory)
editor-revision-history-title = Revision History

# Editor: revision list (RevisionList)
editor-revisions-empty-title = No revisions yet
editor-revisions-empty-hint = Revisions are created when you make changes
editor-revisions-current-version = Current Version
editor-revisions-by = By:
editor-revisions-more-contributors = +{ $count }
editor-revisions-word-count = { $count } { $count ->
    [one] word
   *[other] words
  }
editor-revisions-restore-button = Restore This Version
editor-revisions-restoring = Restoring...
editor-revisions-unknown-user = Unknown
editor-revisions-load-error = Failed to load revisions
editor-revisions-restore-error = Failed to restore revision
editor-revisions-just-now = Just now
editor-revisions-minutes-ago = { $minutes }m ago
editor-revisions-hours-ago = { $hours }h ago
editor-revisions-days-ago = { $days }d ago
editor-revisions-confirm-title = Restore Revision?
editor-revisions-confirm-body = This will restore the ticket to revision { $revision }. This action will replace the current content with the selected revision.
editor-revisions-confirm-note = Note: A new revision will be created so you can always undo this change.
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
ticket-media-attachment-format-unsupported = This image format is not supported by your browser
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
ticket-media-voice-mic-error = Could not access microphone. Please check your permissions.

# Ticket media: video player (VideoPlayer)
ticket-media-video-play = Play
ticket-media-video-pause = Pause
ticket-media-video-mute = Mute
ticket-media-video-unmute = Unmute
ticket-media-video-fullscreen-enter = Enter fullscreen
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
ticket-media-file-thumbnail-error = Failed to generate thumbnail
ticket-media-file-image-error = Failed to load image
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
ticket-picker-linked-error = Failed to load tickets
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
ticket-picker-canned-load-error = Failed to load templates
ticket-picker-canned-search-placeholder = Search canned responses…
ticket-picker-canned-search-aria = Search canned responses
ticket-picker-canned-no-matches = No matches for "{ $query }"
ticket-picker-canned-missing-vars = This template uses {"{{"}{ $names }{"}}"} which the current ticket has no value for. Those slots will be empty.

# Ticket picker: device modal (DeviceModal)
ticket-picker-device-title = Add asset
ticket-picker-device-name-label = Name
ticket-picker-device-name-placeholder = Enter asset name
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
ticket-picker-device-add = Add asset

# Document icon selector (DocumentIconSelector)
doc-icon-selector-trigger-aria = Select document icon
doc-icon-selector-search-placeholder = Search icons...
doc-icon-selector-empty = No icons found
doc-icon-selector-footer-hint = Click an icon to select
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
settings-profile-banner-change = Change banner image
settings-profile-avatar-change = Change profile photo
settings-profile-change-photo = Change Photo
settings-profile-name-placeholder = Enter name...
settings-profile-pronouns-label = Pronouns
settings-profile-pronouns-placeholder = Add pronouns (e.g., he/him, she/her, they/them)
settings-profile-save = Save
settings-profile-signature-label = Email signature
settings-profile-signature-hint-prefix = Appended to your outbound replies on channel-originated tickets (email). Separator is standard
settings-profile-signature-hint-suffix = .
settings-profile-signature-variables-hint = Variables (filled in per reply):
settings-profile-signature-placeholder = Agent Name
    IT Support
settings-profile-unknown-user = Unknown User
settings-profile-role-developer = Developer
settings-profile-role-admin = Administrator
settings-profile-role-technician = Agent
settings-profile-role-user = User
settings-profile-error-invalid-file = Invalid file
settings-profile-error-process-image = Failed to process image
settings-profile-error-user-uuid-missing = User UUID not found
settings-profile-error-not-authenticated = User not authenticated
settings-profile-avatar-upload-success = Profile picture updated successfully
settings-profile-banner-upload-success = Cover image updated successfully
settings-profile-avatar-upload-error = Failed to upload avatar
settings-profile-banner-upload-error = Failed to upload banner
settings-profile-avatar-update-error = Failed to update avatar
settings-profile-banner-update-error = Failed to update banner
settings-profile-name-update-success = Name updated successfully
settings-profile-name-update-error = Failed to update name
settings-profile-pronouns-update-success = Pronouns updated successfully
settings-profile-pronouns-update-error = Failed to update pronouns
settings-profile-signature-update-success = Signature updated
settings-profile-signature-update-error = Failed to update signature

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
settings-notifications-type-ticket-assigned-description = When you are assigned to a ticket
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
settings-notifications-type-asset-low-stock-name = Low Stock Alert
settings-notifications-type-asset-low-stock-description = When a stock-tracked asset drops to or below its configured low-stock threshold
settings-notifications-browser-banner-title = Enable Browser Notifications
settings-notifications-browser-banner-description = Allow browser notifications to receive alerts even when the app isn't in focus.
settings-notifications-browser-banner-enable = Enable Notifications
settings-notifications-browser-enabled-success = Browser notifications enabled
settings-notifications-browser-denied-error = Browser notification permission denied
settings-notifications-quick-settings-title = Quick Settings
settings-notifications-channel-toggle-all-label = All { $channel } Notifications
settings-notifications-column-header = Notification
settings-notifications-load-error = Failed to load notification preferences
settings-notifications-preference-update-success = Preference updated
settings-notifications-preference-update-error = Failed to update preference
settings-notifications-channel-bulk-success = All { $channel } notifications { $state ->
    [enabled] enabled
   *[disabled] disabled
}
settings-notifications-info-footer = Email notifications are rate limited to prevent inbox flooding. You'll receive at most one email per ticket every 5 minutes.

# Settings: passkeys (PasskeySettings)
settings-passkey-section-title = Passkeys
settings-passkey-empty-title = No passkeys registered
settings-passkey-empty-admin-description = This user has not registered any passkeys.
settings-passkey-empty-self-description = Sign in with biometrics or a security key instead of a password
settings-passkey-add-button = Add Passkey
settings-passkey-add-another-button = Add Another Passkey
settings-passkey-synced-badge = Synced
settings-passkey-last-used = Last used { $date }
settings-passkey-never-used = Never used
settings-passkey-rename-tooltip = Rename passkey
settings-passkey-delete-tooltip = Delete passkey
settings-passkey-admin-info = Passkey registration requires the account owner's biometrics or security key.
settings-passkey-unsupported-title = Browser Not Supported
settings-passkey-unsupported-description = Your browser does not support passkeys (WebAuthn). Please use a modern browser like Chrome, Safari, Firefox, or Edge.
settings-passkey-admin-load-error = Failed to load passkeys for this user
settings-passkey-admin-delete-success = Passkey has been deleted
settings-passkey-admin-delete-error = Failed to delete passkey
settings-passkey-add-modal-title = Add Passkey
settings-passkey-add-modal-description = Give your passkey a name to help you identify it later. Your device will prompt you to create the passkey.
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
settings-passkey-delete-modal-confirm-prefix = Are you sure you want to delete
settings-passkey-delete-modal-confirm-suffix = ? You will no longer be able to use this passkey to sign in.
settings-passkey-delete-modal-password-label = Enter your password to confirm
settings-passkey-delete-modal-password-placeholder = Your password
settings-passkey-delete-modal-confirm = Delete Passkey
settings-passkey-admin-delete-modal-confirm-prefix = Are you sure you want to delete
settings-passkey-admin-delete-modal-confirm-suffix = ? This user will no longer be able to sign in with this passkey.
settings-passkey-admin-delete-modal-deleting = Deleting...

# Settings: active sessions (SessionsSettings)
settings-sessions-section-title = Active sessions
settings-sessions-empty = No active sessions found.
settings-sessions-unknown-device = Unknown device
settings-sessions-on = on
settings-sessions-current-badge = This session
settings-sessions-this-device = Current device
settings-sessions-last-active = Active { $time }
settings-sessions-signed-in = Signed in { $date }
settings-sessions-revoke = Sign out
settings-sessions-revoke-aria = Sign out { $device }
settings-sessions-revoke-success = Session signed out.
settings-sessions-revoke-error = Couldn't sign out that session. Try again.
settings-sessions-revoke-others-button = Sign out all other sessions
settings-sessions-revoke-others-modal-title = Sign out all other sessions?
settings-sessions-revoke-others-modal-description = This signs out every device except this one. Confirm your identity to continue.
settings-sessions-revoke-others-stepup-password = Confirm your password to continue
settings-sessions-revoke-others-stepup-mfa = Enter an authentication code to continue
settings-sessions-modal-cancel = Cancel
settings-sessions-revoke-others-confirm = Sign out others
settings-sessions-revoke-others-success = Signed out all other sessions.
settings-sessions-revoke-others-error = Couldn't sign out the other sessions. Check your credentials and try again.

# Settings: unsaved-changes navigate-away guard (shared)
settings-unsaved-leave-title = Discard unsaved changes?
settings-unsaved-leave-message = You have unsaved changes on this page. Leave without saving?
settings-unsaved-leave-confirm = Discard
settings-unsaved-leave-cancel = Keep editing

# Auth: passkey setup (PasskeySetup)
auth-passkey-setup-unsupported-title = Passkeys Not Available
auth-passkey-setup-unsupported-insecure = Passkeys require a secure connection (HTTPS). You're currently on an insecure connection.
auth-passkey-setup-unsupported-browser = Your browser does not support passkeys. Please use a modern browser like Chrome, Safari, Firefox, or Edge, or choose the authenticator app option instead.
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
auth-passkey-setup-error-generic = Failed to register passkey
auth-passkey-setup-success-message = Passkey created successfully
auth-passkey-setup-backup-codes-title = Save Your Recovery Codes
auth-passkey-setup-backup-codes-description = If you lose access to your passkey, you can use one of these codes to sign in. Each code can only be used once.
auth-passkey-setup-backup-codes-copy = Copy
auth-passkey-setup-backup-codes-copied = Copied!
auth-passkey-setup-backup-codes-download = Download
auth-passkey-setup-backup-codes-acknowledge = I've saved my recovery codes
auth-passkey-setup-backup-file-title = Nosdesk Recovery Codes
auth-passkey-setup-backup-file-intro = Save these codes in a safe place. Each code can only be used once.
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
settings-emails-confirm-message = { $email } will no longer be associated with this account.
settings-emails-confirm-label = Remove
settings-emails-error-required = Email address is required
settings-emails-error-invalid-format = Invalid email format
settings-emails-add-success = Email address added successfully
settings-emails-add-error = Failed to add email address
settings-emails-set-primary-success = Set { $email } as primary email
settings-emails-set-primary-error = Failed to set email as primary
settings-emails-delete-success = Email address removed successfully
settings-emails-delete-error = Failed to delete email address
# Docs: article card (ArticleCard)
docs-article-card-updated = Updated { $date }
docs-article-card-edit = Edit Article

# Docs: collection manager modal (CollectionManager)
docs-collection-manager-title = Manage Collections
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
docs-collection-visibility-title = Collection Access
docs-collection-visibility-description = Select which groups and users can access this collection. Empty selection means the collection is public (visible to everyone).
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
docs-actions-menu-trash = Move to Trash
docs-actions-menu-trash-confirm = Confirm trash?
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
docs-nav-confirm-delete-collection-message = Pages in this collection will be moved to the trash. You can restore them from there.
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
docs-nav-menu-trash = Move to Trash
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
docs-toc-item-untitled = Untitled Page

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
docs-edit-collection-color = Color
docs-edit-collection-description = Short description
docs-edit-collection-description-placeholder = Optional tagline shown above the collection's overview
docs-edit-collection-description-help = The full overview is edited in-place on the collection landing page.
docs-edit-collection-hide-titles-aria = Hide page titles from non-members
docs-edit-collection-hide-titles-label = Hide page titles from non-members
docs-edit-collection-hide-titles-help = Cross-collection wikilinks render as "Restricted page" for viewers without access, instead of leaking the title. Recommended for sensitive collections.
docs-edit-collection-name-required = Name is required.
docs-edit-collection-save-error = Failed to save. Try again.
docs-edit-collection-cancel = Cancel
docs-edit-collection-save = Save changes
docs-edit-collection-saving = Saving...

# Docs: move document modal (MoveDocumentModal)
docs-move-title = Move Document
docs-move-search-placeholder = Search pages...
docs-move-root-label = Root level (no parent)
docs-move-current-badge = Current
docs-move-empty-search = No matching pages found.
docs-move-empty = No pages available.
docs-move-cancel = Cancel
docs-move-action = Move
docs-move-moving = Moving...

# Docs: page permissions modal (PagePermissionsModal)
docs-page-permissions-title = Page Permissions
docs-page-permissions-mode-inherit = Inherit from collections
docs-page-permissions-mode-custom = Custom access
docs-page-permissions-inherit-description = This page inherits visibility from its collections. Users who can access any of the page's collections can see this page.
docs-page-permissions-no-collections = Not in any collection, visible to everyone.
docs-page-permissions-custom-description = Select which groups and users can access this page. This overrides collection-level permissions.
docs-page-permissions-picker-placeholder = Search users and groups...
docs-page-permissions-no-selection-warning = No groups or users selected, no one except admins will be able to see this page.
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
ticket-detail-source-tooltip = Opened via { $provider }. Replies are relayed back through the thread.
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
ticket-detail-claim-title = Assign this ticket to yourself
ticket-detail-sla-label = SLA
ticket-detail-sla-paused-target = target { $target }
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
ticket-detail-recurrence-custom-note = Custom RRULE in use ({ $rule }). Edit via API.
ticket-detail-recurrence-respawn-note = Closing this ticket spawns the next occurrence.
ticket-detail-category-placeholder = Select category...
ticket-detail-cycle-label = Cycle
ticket-detail-cycle-tooltip = Cycle { $name } ({ $state })
ticket-detail-resolution-label = Resolution
ticket-detail-resolution-closed = Closed
ticket-detail-resolution-draft-from-notes = Draft from notes
ticket-detail-resolution-draft-from-notes-title = { $count ->
    [one] Append { $count } internal note to the resolution draft
   *[other] Append { $count } internal notes to the resolution draft
  }
ticket-detail-resolution-placeholder = What fixed this?
ticket-detail-audit-created = Created
ticket-detail-audit-created-by = Created by { $name }
ticket-detail-audit-modified = Updated
ticket-detail-audit-closed = Closed
ticket-detail-audit-closed-by = Closed by { $name }
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
ticket-detail-print-qr-alt = Ticket QR Code
ticket-detail-print-qr-label = Scan to open

# Ticket: comments & attachments composer (CommentsAndAttachments).
ticket-comments-section-title = Comments and Attachments
ticket-comments-drop-files = Drop files here
ticket-comments-internal-banner = Visible to staff only. Not sent through the ticket's channel.
ticket-comments-placeholder-public = Add a new comment...
ticket-comments-placeholder-internal = Note for the team…
ticket-comments-record-voice = Record voice note
ticket-comments-upload-file = Upload file
ticket-comments-visibility-group = Reply visibility
ticket-comments-public-reply = Public reply
ticket-comments-public-reply-title = Sent to the requester through the ticket's channel
ticket-comments-internal-note = Internal note
ticket-comments-internal-note-title = Visible only to agents; not relayed back through the ticket's channel
ticket-comments-submit-reply = Add reply
ticket-comments-submit-note = Add note
ticket-comments-voice-note-filename = Voice Note { $date }
ticket-comments-filter-group = Comment visibility filter
ticket-comments-filter-all = All ({ $count })
ticket-comments-filter-public = Public ({ $count })
ticket-comments-filter-internal = Internal ({ $count })
ticket-comments-badge-internal = Internal
ticket-comments-badge-forwarded = Forwarded
ticket-comments-badge-forwarded-title = An agent forwarded this email into the helpdesk
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
ticket-activity-load-error = Failed to load activity
ticket-activity-load-more-error = Failed to load more activity
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
ticket-activity-made-changes = made { $count } { $count ->
    [one] change
   *[other] changes
}
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
ticket-activity-phrase-merged = merged { $count ->
    [one] 1 ticket
   *[other] { $count } tickets
  } into this one
ticket-activity-phrase-merged-into = merged this ticket into #{ $target_id }
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
ticket-field-watchers-notify-internal-hint = Get pinged on private staff replies
ticket-field-watchers-public-only = Public replies only
ticket-field-watchers-prefs-title = Notification preferences
ticket-field-watchers-toggle-on = ON
ticket-field-watchers-toggle-off = OFF
ticket-field-watchers-pref-load-error = Failed to load preference
ticket-field-watchers-pref-save-error = Failed to save preference
ticket-field-watchers-overflow-title = { $count ->
    [one] { $count } more
   *[other] { $count } more
  }

# Ticket: device chip row (TicketDevicesField).
ticket-field-devices-label = Assets
ticket-field-devices-add = Add asset
ticket-field-devices-detach = Detach asset
ticket-field-devices-fallback-name = Asset #{ $id }
ticket-field-devices-title-with-model = { $hostname } · { $model }

# Ticket: asset usage panel (TicketAssetUsage). Surfaces the
# per-ticket asset_usage_log ledger plus inline forms for
# recording new consumption against stock-tracked linked assets.
ticket-asset-usage-heading = Asset usage
ticket-asset-usage-empty-no-stock = No stock-tracked assets linked to this ticket.
ticket-asset-usage-empty-no-history = No usage recorded yet.
ticket-asset-usage-quantity-placeholder = Used (in { $unit })
ticket-asset-usage-notes-placeholder = Notes (optional)
ticket-asset-usage-load-failed = Failed to load usage history
ticket-asset-usage-record-failed = Failed to record usage

# Asset detail: usage history panel (AssetUsageHistory).
asset-usage-history-heading = Usage history
asset-usage-history-empty = No usage recorded yet.
asset-usage-history-load-failed = Failed to load usage history
asset-usage-history-load-more = Load more
asset-usage-history-loading = Loading…
asset-usage-history-ticket-link = Ticket #{ $id }
asset-usage-history-ad-hoc = Ad-hoc consumption

# Asset detail: ad-hoc consumption recording (AssetUsageHistory record form).
asset-usage-record-heading = Record consumption
asset-usage-record-on-hand = on hand
asset-usage-record-quantity-placeholder = Quantity ({ $unit })
asset-usage-record-notes-placeholder = Notes (optional)
asset-usage-record-submit = Record

# Phase H — restock affordance on the asset usage panel.
asset-usage-record-submit-usage-title = Record consumption (decrement stock)
asset-usage-record-submit-restock = + Restock

# Stock audit (physical count) on the asset history panel.
asset-audit-record-heading = Audit count
asset-audit-record-hint = enter the physical count, system corrects to match
asset-audit-record-placeholder = Counted ({ $unit })
asset-audit-record-notes-placeholder = Notes (optional)
asset-audit-record-submit = Save audit
asset-audit-record-failed = Failed to record audit
asset-audit-history-label = Audit
asset-audit-history-previous = was { $previous }
asset-usage-record-submit-restock-title = Record restock (increment stock)
asset-usage-record-failed = Failed to record usage

# Asset detail: stock tracking section + low-stock indicator (Phase G).
asset-detail-section-stock = Stock tracking
asset-detail-field-quantity = On-hand quantity
asset-detail-field-quantity-placeholder = e.g. 25
asset-detail-field-unit = Unit
asset-detail-field-unit-placeholder = e.g. m, L, pcs
asset-detail-field-low-stock-threshold = Low-stock threshold
asset-detail-field-low-stock-threshold-placeholder = e.g. 5
asset-detail-field-low-stock-threshold-help = Show a warning and broadcast an event when stock falls to or below this value.

# Editable kind / attributes affordance on existing assets.
asset-detail-attributes-save = Save attributes
asset-detail-attributes-discard = Discard
asset-detail-attributes-save-failed = Failed to save attributes
asset-detail-kind-change-confirm = Change kind to { $newKind }? Current attributes will be cleared, you can re-enter them against the new kind schema.
asset-detail-kind-change-title = Change asset kind?
asset-detail-kind-change-confirm-label = Change kind
asset-detail-kind-change-failed = Failed to change kind
asset-detail-low-stock-warning = Low stock: { $quantity } { $unit } remaining (threshold { $threshold }).
asset-low-stock-toast-title = Low stock: { $name }
asset-low-stock-toast-body = { $quantity } { $unit } remaining (threshold { $threshold }).

# Asset list: low-stock badge surfaced on each row.
assets-list-low-stock-badge = Low stock
assets-list-low-stock-tooltip = { $quantity } { $unit } remaining (threshold { $threshold }).

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

# Ticket: chips/badges (PropertyChip, LinkedTicketChip, ProjectChip,
# SidebarCard, TicketGapFlag, LinkedTicketPreview, DeviceDetails).
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
ticket-chip-device-remove = Remove asset
ticket-chip-device-view-title = View asset
ticket-chip-device-unnamed = Unnamed asset
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
# Admin: env-config notice (EnvConfigNotice) - shown above admin
# panels that are configured by environment variables.
admin-env-notice-title = Configuration via Environment Variables
admin-env-notice-prefix = Settings are configured through environment variables in your
admin-env-notice-suffix = file or Docker environment.

# Admin: system info card (SystemInfoCard) - version, environment,
# uptime, and update-available banner.
admin-system-info-title = System Information
admin-system-info-version = Version
admin-system-info-environment = Environment
admin-system-info-uptime = Uptime
admin-system-info-update-to = Update to { $version }
admin-system-info-uptime-days = { $count }d
admin-system-info-uptime-hours = { $count }h
admin-system-info-uptime-minutes = { $count }m
admin-system-info-uptime-seconds = { $count }s

# Admin: category editor (CategoryEditPanel) - create / edit
# categories with name, description, icon, color, visibility.
admin-categories-edit-title-edit = Edit Category
admin-categories-edit-title-create = Create Category
admin-categories-edit-delete-tooltip = Delete category
admin-categories-edit-close-tooltip = Close panel
admin-categories-edit-name-label = Name
admin-categories-edit-name-placeholder = Enter category name
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
admin-categories-edit-color-label = Color
admin-categories-edit-active-label = Active
admin-categories-edit-visibility-label = Visible to Groups
admin-categories-edit-visibility-hint = (leave empty for public)
admin-categories-edit-visibility-toggle-aria = Toggle visibility for { $name }
admin-categories-edit-member-count = { $count ->
    [one] { $count } member
   *[other] { $count } members
    }
admin-categories-edit-no-groups = No groups available. Create groups first.
admin-categories-edit-cancel = Cancel
admin-categories-edit-save = Save Changes
admin-categories-edit-create = Create Category

# Admin: group configuration panel (GroupConfigurationPanel) -
# edit a group's name / color / members / devices / included
# groups. Read-only when the group is managed by Microsoft Entra.
admin-groups-config-subtitle = Group Configuration
admin-groups-config-delete-tooltip = Delete group
admin-groups-config-close-tooltip = Close panel
admin-groups-config-source-microsoft = Microsoft Entra ID
admin-groups-config-managed-by = Managed by { $source }
admin-groups-config-last-synced = Last synced { $date }
admin-groups-config-unmanage = Unmanage
admin-groups-config-unmanage-processing = Processing...
admin-groups-config-sync-settings = Sync Settings
admin-groups-config-general = General Information
admin-groups-config-name-label = Name
admin-groups-config-name-placeholder = Enter group name
admin-groups-config-description-label = Description
admin-groups-config-description-placeholder = Optional description
admin-groups-config-color-label = Color
admin-groups-config-save-changes = Save Changes
admin-groups-config-members = Members
admin-groups-config-no-members = No members
admin-groups-config-devices = Assets
admin-groups-config-device-sn = SN: { $sn }
admin-groups-config-no-devices = No assets
admin-groups-config-included-in = Included In
admin-groups-config-included-groups = Included Groups
admin-groups-config-includes-hint = Members of included groups are treated as members of this group for visibility, access, and assignment.
admin-groups-config-source-direct = Direct
admin-groups-config-source-via = via
admin-groups-config-source-also-via = also via
admin-groups-config-section-assigned = Assigned
admin-groups-config-section-included-via = Included via Groups
admin-groups-config-section-not-assigned = Not Assigned
admin-groups-config-search-users = Search users...
admin-groups-config-search-devices = Search assets by name, hostname, serial...
admin-groups-config-search-groups = Search groups...
admin-groups-config-no-users-found = No users found
admin-groups-config-no-devices-found = No assets found
admin-groups-config-no-groups-found = No groups found
admin-groups-config-synced-badge = Synced
admin-groups-config-synced-intune-tooltip = Synced from Microsoft Intune
admin-groups-config-selected-count = { $count } selected
admin-groups-config-member-count = { $count ->
    [one] { $count } member
   *[other] { $count } members
    }
admin-groups-config-save-members = Save Members
admin-groups-config-save-devices = Save assets
admin-groups-config-save-includes = Save Included Groups
admin-groups-config-not-found = Group not found
admin-groups-config-cancel = Cancel
admin-groups-config-delete-title = Delete Group
admin-groups-config-delete-confirm = Delete Group
admin-groups-config-delete-prompt-prefix = Are you sure you want to delete the group
admin-groups-config-delete-prompt-suffix = ? This will remove all member associations but will not delete the users.
admin-groups-config-unmanage-title = Unmanage group?
admin-groups-config-unmanage-title-named = Unmanage { $name }?
admin-groups-config-unmanage-message = The group will no longer sync with Microsoft Entra ID. Manual edits become allowed, but existing sync history is preserved.
admin-groups-config-error-invalid-id = Invalid group ID
admin-groups-config-error-load = Failed to load group details
admin-groups-config-error-name-required = Group name is required
admin-groups-config-error-save = Failed to save group
admin-groups-config-error-members = Failed to update members
admin-groups-config-error-devices = Failed to update assets
admin-groups-config-error-includes = Failed to update included groups
admin-groups-config-error-delete = Failed to delete group
admin-groups-config-error-unmanage = Failed to unmanage group
admin-groups-config-success-updated = Group updated successfully
admin-groups-config-success-members = Members updated successfully
admin-groups-config-success-devices = Assets updated successfully
admin-groups-config-success-includes = Included groups updated successfully
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
views-ticket-row-cycle-tooltip = Belongs to a cycle
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
views-ticket-preview-sla-response = Response
views-ticket-preview-sla-resolution = Resolution
views-ticket-preview-activity = Activity
views-ticket-preview-last-activity = Last activity
views-ticket-preview-created = Created
views-ticket-preview-affected-devices = Affected assets
views-ticket-preview-more-devices = { $count ->
    [one] +{ $count } more
   *[other] +{ $count } more
    }
views-ticket-preview-view-full = View description, comments, and assets

# Ticket heatmap (TicketHeatmap) - 365-day activity grid on the
# dashboard. Day labels are abbreviated.
ticket-heatmap-title-closed = Closed Tickets
ticket-heatmap-title-activity = Ticket Activity
ticket-heatmap-error-load = Failed to load ticket data. Please try again.
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

# Views: add-filter menu (AddFilterMenu) - facet picker + value
# picker. Facet labels override the FACET_META English fallback.
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

# Views: filter value list (FilterValueList) - searchable multi-
# select inside filter popovers.
views-filter-value-search-placeholder = Search…
views-filter-value-no-matches = No matches
views-filter-value-no-options = No options
views-filter-value-clear = Clear

# Views: display menu (DisplayMenu) - density, grouping, column
# visibility. Value keys ('compact', 'status', etc.) are stable;
# only the labels localise.
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

# Views: tab strips (TicketsViewTabs, ProjectTabBar) - aria-label
# only; tab text comes from props.
views-tab-bar-aria = View
views-project-tab-aria = Project view
views-project-tab-board = Board
views-project-tab-gantt = Gantt
views-project-tab-cycles = Cycles

# Views: saved-view editor modal (SavedViewEditorModal)
views-saved-editor-title = Edit view
views-save-as-title = Save view as
views-save-as-name-label = Name
views-save-as-save = Save
views-save-as-saving = Saving
views-save-as-cancel = Cancel
views-save-trigger = Save view as
views-save-default-suffix = (copy)
views-save-as-success = Saved view "{ $name }"
views-save-as-error = Could not save view
views-saved-editor-rename-error = Could not rename view
views-saved-editor-delete-error = Could not delete view
views-asset-switcher-placeholder = Asset views
views-user-switcher-placeholder = User views
views-saved-editor-name-label = Name
views-saved-editor-delete = Delete view
views-saved-editor-cancel = Cancel
views-saved-editor-save = Save
views-saved-editor-saving = Saving
views-saved-editor-confirm-title = Delete view?
views-saved-editor-confirm-message = Delete "{ $name }"? This can't be undone, recreate the view if you need it back.

# Views: filter pill (FilterPill)
views-filter-pill-remove-tooltip = Remove { $label } filter
views-filter-pill-search-title-placeholder = Search title…

# Views: view switcher (ViewSwitcher)
views-view-switcher-placeholder = View
views-view-switcher-edit-view = Edit view…

# Dashboard: user-assigned tickets widget (UserAssignedTickets) -
# Status filter, sort dropdown, high-priority / new-activity
# toggles. Value strings stay as canonical filter / sort keys.
user-assigned-tickets-title-assigned = Assigned Tickets
user-assigned-tickets-title-requested = Requested Tickets
user-assigned-tickets-empty-title-assigned = No assigned tickets
user-assigned-tickets-empty-title-requested = No requested tickets
user-assigned-tickets-empty-current = You're all caught up!
user-assigned-tickets-error-assigned = Failed to load assigned tickets
user-assigned-tickets-error-requested = Failed to load requested tickets
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

# Dashboard: recent tickets widget (RecentTickets) - right-rail
# list of recently-viewed tickets with a context menu.
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
plugin-detail-error-load = Failed to load plugin
plugin-detail-error-save = Failed to save settings. Try again.
plugin-detail-error-toggle = Failed to toggle plugin
plugin-detail-error-uninstall = Failed to uninstall plugin
plugin-detail-uninstall-title = Uninstall plugin
plugin-detail-uninstall-prompt-prefix = Uninstall
plugin-detail-uninstall-prompt-mid = ? The plugin's
plugin-detail-uninstall-prompt-suffix = policy decides whether its data is preserved or removed.
plugin-detail-uninstall-cancel = Cancel
plugin-detail-uninstall-confirm = Uninstall

# Plugin admin: sideload view (PluginSideloadView)
plugin-sideload-back = Back to plugins
plugin-sideload-title = Sideload signed zip
plugin-sideload-intro-prefix = For plugins that aren't in the registry yet. The bundle must be signed by a registered publisher or this instance's local signing key; unsigned uploads are refused. Looking for an official plugin? Browse the
plugin-sideload-intro-link = registry
plugin-sideload-intro-suffix = first.
plugin-sideload-dropzone-aria = Choose plugin zip file
plugin-sideload-choose-different = Choose a different file
plugin-sideload-drop-here = Drop your plugin zip here
plugin-sideload-or-browse = or click to browse
plugin-sideload-warning-title = Only sideload plugins from sources you trust.
plugin-sideload-warning-prefix = A signature confirms the bundle hasn't been tampered with after signing, but it doesn't vouch for the publisher's intent. An installed plugin runs in the admin UI with access to your session. Prefer the
plugin-sideload-warning-link = registry
plugin-sideload-warning-suffix = for vetted publishers, and review the source of anything you sideload.
plugin-sideload-cancel = Cancel
plugin-sideload-install = Install plugin
plugin-sideload-installing = Installing...
plugin-sideload-error-not-zip = Please select a .zip file
plugin-sideload-error-too-large = File must be less than 2 MB
plugin-sideload-error-install-failed = Failed to install plugin
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
guest-submit-disabled-title = Ticket submission is not available
guest-submit-disabled-message = Guest ticket submission is currently disabled. Please sign in if you have an account.
guest-submit-verify-title = Check your inbox
guest-submit-verify-message-prefix = Click the confirmation link we sent to
guest-submit-verify-message-suffix = to release your ticket and set up your portal.
guest-submit-verify-spam-hint = Didn't get it? Check spam, then try again in a few minutes.
guest-submit-another = Submit another ticket
guest-submit-success-title = Ticket received
guest-submit-success-email-prefix = We've sent a confirmation to
guest-submit-success-email-suffix = with a link to sign in and track progress.
guest-submit-success-no-email = Your ticket has been logged. Our team will follow up by email.
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
guest-submit-error-name = Please enter your name.
guest-submit-error-email = Please enter a valid email address.
guest-submit-error-title = Please enter a subject.
guest-submit-error-description = Please describe the issue.
guest-submit-error-uploads-pending = Please wait for file uploads to finish.
guest-submit-error-rate-limited = Too many submissions from your network. Please try again later.
guest-submit-error-disabled = Ticket submission has been disabled.
guest-submit-error-account-exists = An account exists for this email. Please sign in to submit a ticket.
guest-submit-error-generic = Failed to submit ticket. Please try again.
guest-submit-error-network = Network error. Please try again.
guest-submit-attach-error-max = Up to { $max } attachments.
guest-submit-attach-error-too-large = { $name } is over { $size }MB.
guest-submit-attach-error-rate-limited = Too many uploads from your network. Try again later.
guest-submit-attach-error-too-large-server = { $name } is too large.
guest-submit-attach-error-disabled = Attachments are not accepted right now.
guest-submit-attach-error-generic = Upload failed. Try again.
guest-submit-attach-error-network = Network error uploading file.
guest-submit-size-bytes = { $bytes } B
guest-submit-size-kb = { $value } KB
guest-submit-size-mb = { $value } MB

# Guest ticket status
guest-status-loading-aria = Loading ticket
guest-status-disabled-title = Status lookup is not available
guest-status-disabled-message = Guest ticket status lookup is currently disabled.
guest-status-ticket-number = Ticket #{ $id }
guest-status-priority = Priority
guest-status-opened = Opened
guest-status-last-updated = Last updated
guest-status-closed = Closed
guest-status-reply-prefix = Need to reply?
guest-status-reply-suffix = to add a comment.
guest-status-not-found-title = Ticket not found
guest-status-not-found-message = The link may have expired or been mistyped.

# Public docs list
public-docs-loading-aria = Loading documentation
public-docs-disabled-title = Documentation is not available
public-docs-disabled-message = Public documentation is currently disabled.
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
public-doc-not-found-message = It may have been moved or set to private.
public-doc-back-to-docs = Back to docs

# Help page
help-disabled-title = Help page is not available
help-disabled-message = The self-service help page is currently disabled.
help-heading = How can we help?
help-tagline = Here are a few things you can do without an account.
help-card-submit-title = Submit a ticket
help-card-submit-desc = Report an issue and we'll get back to you by email.
help-card-docs-title = Browse documentation
help-card-docs-desc = Public articles, guides, and how-tos.
help-card-reset-title = Reset your password
help-card-reset-desc = Lost access to your account? Start here.
help-card-signin-title = Sign in
help-card-signin-desc = Already have an account?

# Settings: appearance pane (AppearanceSettings) — theme picker,
# device-only sync toggle, accessibility (colorblind-safe shapes),
# and compact-view density toggle. Strings are flat status text;
# any pluralisation in this block is handled by Fluent selectors.
settings-appearance-title = Appearance
settings-appearance-theme-heading = Theme
settings-appearance-theme-description = Choose your preferred color scheme
settings-appearance-device-local-label = Device-only theme
settings-appearance-device-local-description = Don't sync theme across devices (e.g., use E-Paper theme on your tablet while keeping dark mode on your laptop)
settings-appearance-section-automatic = Automatic
settings-appearance-section-light = Light Themes
settings-appearance-section-dark = Dark Themes
settings-appearance-red-horizon-easter-egg = Why would you do this to them 😭
settings-appearance-accessibility-heading = Accessibility
settings-appearance-accessibility-description = Improve readability and visual distinction
settings-appearance-colorblind-label = Color blind friendly mode
settings-appearance-colorblind-description-monochrome = Always enabled for monochromatic themes like E-Paper and Red Horizon
settings-appearance-colorblind-description-default = Use distinct shapes for status indicators instead of relying only on colors
settings-appearance-display-heading = Display
settings-appearance-display-description = Adjust layout preferences
settings-appearance-compact-label = Compact view
settings-appearance-compact-description = Reduce spacing between elements for a denser layout
settings-appearance-theme-changed = Theme changed to { $name }
settings-appearance-theme-changed-device-only = Theme changed to { $name } (device only)
settings-appearance-theme-save-failed = Failed to save theme preference
settings-appearance-colorblind-toggled = Color blind friendly mode { $state ->
    [enabled] enabled
   *[disabled] disabled
}
settings-appearance-device-local-toggled = Device-only theme { $state ->
    [enabled] enabled
   *[disabled] disabled
}
settings-appearance-compact-toggled = Compact view { $state ->
    [enabled] enabled
   *[disabled] disabled
}
settings-appearance-system-theme-name = System

# Settings: ThemeCard — individual theme preview tile rendered
# inside the appearance grid. Only the visible label and the
# "system" fallback name are localised; the inline preview is
# pure CSS.
settings-appearance-card-system-name = System

# Settings: security pane (SecuritySettings) — password change
# for self and admin reset for another user. Form fields,
# validation hints, and the two server-status messages.
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
settings-security-error-mismatch = Passwords do not match
settings-security-submit-change = Change Password
settings-security-submit-reset = Reset Password
settings-security-error-form-invalid = Please fill in all fields correctly
settings-security-success-changed = Password changed successfully
settings-security-error-change-failed = Failed to change password. Please check your current password.
settings-security-success-reset = Password has been reset for this user
settings-security-error-reset-failed = Failed to reset password

# OAuth callback view + card (AuthCallbackCard). Renders the
# in-flight spinner, three error variants (already-connected,
# invalid-request, generic), and a transient success state.
# $provider is the human-readable IdP label (Microsoft / SSO).
auth-callback-loading-default = Completing sign-in...
auth-callback-loading-processing = Processing authentication...
auth-callback-loading-success = Success! Redirecting...
auth-callback-loading-subtitle = Please wait while we complete authentication
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
auth-callback-error-generic-message = An unexpected error occurred during authentication
auth-callback-error-status-prefix = Status: { $status }
auth-callback-already-title = Account Already Connected
auth-callback-already-message = This { $provider } account is already linked to another user in the system.
auth-callback-already-suggestion-microsoft = Try signing in with a different { $provider } account, or contact your administrator.
auth-callback-already-suggestion-generic = Try signing in with a different account, or contact your administrator.
auth-callback-invalid-title = Authentication Failed
auth-callback-invalid-message = The authentication request was invalid or has expired.
auth-callback-invalid-suggestion-microsoft = Please try connecting your { $provider } account again.
auth-callback-invalid-suggestion-generic = Please try signing in again.
auth-callback-generic-title = Authentication Failed
auth-callback-generic-suggestion = Please try again or contact support if the problem persists.
auth-callback-action-try-different = Try a Different Account
auth-callback-action-back-settings = Back to Settings
auth-callback-action-return-login = Return to Login
auth-callback-action-try-again = Try Again

# Dashboard widgets
dashboard-widget-shell-action-view-all = View all
dashboard-widget-shell-empty-title-default = Nothing here yet.

# Three-taxonomy empty-state defaults. Per-widget overrides land
# via the shell's `emptyTitle` / `emptyDescription` props; these
# strings are the fallback when a widget passes only `emptyTaxonomy`.
dashboard-widget-shell-empty-never-had-data-title = Nothing here yet.
dashboard-widget-shell-empty-never-had-data-description = New activity will show up automatically.
dashboard-widget-shell-empty-filtered-title = Nothing matches.
dashboard-widget-shell-empty-filtered-description = Loosen the filter to see more.
dashboard-widget-shell-empty-unconfigured-title = Not configured yet.
dashboard-widget-shell-empty-unconfigured-description = An admin sets this up once and the widget fills in.
dashboard-widget-shell-empty-cta-default = Set it up
dashboard-widget-shell-drag-label = Drag { $title }
dashboard-widget-shell-size-group-label = { $title } size
dashboard-widget-shell-size-option-title = Size { $size } of 3
dashboard-widget-shell-hide-label = Hide { $title }
dashboard-widget-shell-loading-label = Loading { $title }

dashboard-edit-bar-editing = Editing dashboard
dashboard-edit-bar-unsaved = Unsaved changes
dashboard-edit-bar-add-widget = Add widget
dashboard-edit-bar-reset = Reset
dashboard-edit-bar-done = Done
dashboard-edit-bar-close = Close
dashboard-edit-bar-discard = Discard
dashboard-edit-bar-undo = Undo
dashboard-edit-bar-redo = Redo
dashboard-edit-bar-undo-tooltip = Undo last change (Cmd-Z)
dashboard-edit-bar-redo-tooltip = Redo (Cmd-Shift-Z)
dashboard-edit-bar-reset-confirm-title = Reset dashboard layout?
dashboard-edit-bar-reset-confirm-message = Your customised layout will be replaced with the default for your role.
dashboard-edit-bar-reset-confirm-label = Reset
dashboard-leave-confirm-title = Discard dashboard changes?
dashboard-leave-confirm-message = You have unsaved changes to your dashboard layout. Leave anyway?
dashboard-leave-confirm-label = Discard
dashboard-edit-bar-save-error-title = Couldn't save dashboard
dashboard-edit-bar-save-error-message = Your changes are still here. Try again, or check your connection.
dashboard-widget-context-menu-aria = { $title } options
dashboard-widget-context-menu-resize-1 = Resize to 1 column (1)
dashboard-widget-context-menu-resize-2 = Resize to 2 columns (2)
dashboard-widget-context-menu-resize-3 = Resize to 3 columns (3)
dashboard-widget-context-menu-hide = Hide widget

dashboard-add-widget-title = Add widget
dashboard-add-widget-all-added = All available widgets are already on your dashboard.
dashboard-add-widget-tab-system = System widgets
dashboard-add-widget-tab-saved-views = Your saved views
dashboard-add-widget-saved-views-loading = Loading saved views...
dashboard-add-widget-saved-views-empty = No chart-backed saved views yet. Build one from the ticket list to pin it here.
dashboard-widget-saved-view-title = Saved view
dashboard-widget-saved-view-description = A chart-backed saved view pinned to your dashboard.
dashboard-saved-view-loading-title = Saved view
dashboard-saved-view-error = Failed to load saved view.
dashboard-saved-view-placeholder = Chart renderer ships in a later wave.
dashboard-saved-view-misconfigured = This view's chart config is missing required fields.
dashboard-kpi-metric-tickets_created = Tickets created
dashboard-kpi-metric-tickets_resolved = Tickets resolved
dashboard-kpi-metric-tickets_open = Tickets open
dashboard-kpi-error = KPI unavailable
dashboard-line-chart-loading = Loading...
dashboard-line-chart-error = Chart unavailable
dashboard-line-chart-empty = No data in this range
dashboard-line-chart-aria-label = Daily time-series
dashboard-bar-priority-low = Low
dashboard-bar-priority-medium = Medium
dashboard-bar-priority-high = High
dashboard-bar-uncategorised = Uncategorised
dashboard-bar-unassigned = Unassigned
dashboard-saved-view-viz-label-list = List
dashboard-saved-view-viz-label-kpi_tile = KPI tile
dashboard-saved-view-viz-label-line = Line chart
dashboard-saved-view-viz-label-horizontal_bar = Horizontal bar
dashboard-saved-view-viz-label-heatmap = Heatmap
dashboard-saved-view-viz-label-leaderboard = Leaderboard
dashboard-saved-view-viz-label-table = Table

dashboard-staff-queue-title = Queue
dashboard-staff-queue-configure-aria = Configure queue metrics
dashboard-staff-queue-configure-title = Configure metrics
dashboard-staff-queue-error = Failed to load queue metrics
dashboard-staff-queue-metric-unassigned-label = Unassigned
dashboard-staff-queue-metric-unassigned-desc = Open, no assignee
dashboard-staff-queue-metric-all-label = All Tickets
dashboard-staff-queue-metric-all-desc = Every status
dashboard-staff-queue-metric-open-label = Open
dashboard-staff-queue-metric-open-desc = Status: open
dashboard-staff-queue-metric-in-progress-label = In Progress
dashboard-staff-queue-metric-in-progress-desc = Currently being worked
dashboard-staff-queue-metric-high-priority-label = High Priority
dashboard-staff-queue-metric-high-priority-desc = High priority, still open
dashboard-staff-queue-metric-closed-today-label = Closed Today
dashboard-staff-queue-metric-closed-today-desc = Closed in the last 24h

dashboard-staff-yours-title = Yours
dashboard-staff-yours-error = Failed to load counts
dashboard-staff-yours-assigned = Assigned
dashboard-staff-yours-open = Open
dashboard-staff-yours-in-progress = In Progress
dashboard-staff-yours-closed = Closed

dashboard-user-summary-title = Summary
dashboard-user-summary-error = Failed to load summary
dashboard-user-summary-requests = Requests
dashboard-user-summary-open = Open
dashboard-user-summary-in-progress = In Progress
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
dashboard-knowledge-gaps-error = Failed to load gaps
dashboard-knowledge-gaps-empty-title = No open gaps
dashboard-knowledge-gaps-empty-description = Tickets flagged for documentation will appear here.
dashboard-knowledge-gaps-signal-count =
    { $count ->
        [one] 1 signal
       *[other] { $count } signals
    }
dashboard-knowledge-gaps-impact-tickets = { $count } tickets
dashboard-knowledge-gaps-impact-searches = { $count } searches
dashboard-knowledge-gaps-impact-tooltip-tickets = { $count } tickets representing demand for this doc
dashboard-knowledge-gaps-impact-tooltip-searches = { $count } searches representing demand for this doc

dashboard-channel-health-title = Channel Health
dashboard-channel-health-action = Manage
dashboard-channel-health-error = Failed to load channels
dashboard-channel-health-empty-title = No channels configured
dashboard-channel-health-empty-description = Add an email channel to ingest tickets.
dashboard-channel-health-status-disabled = Disabled
dashboard-channel-health-status-error = Error
dashboard-channel-health-status-healthy = Healthy
dashboard-channel-health-polled = polled { $time }
dashboard-channel-health-never-polled = never polled

dashboard-my-assets-title = My assets
dashboard-my-assets-error = Failed to load assets
dashboard-my-assets-empty-title = No assets assigned
dashboard-my-assets-empty-description = Assets linked to your account will show here.
dashboard-my-assets-unknown-model = Unknown model

dashboard-recently-viewed-title = Recently Viewed
dashboard-recently-viewed-error = Failed to load recently viewed
dashboard-recently-viewed-empty-title = Nothing here yet
dashboard-recently-viewed-empty-description = Tickets you open will show up here.

dashboard-starred-docs-title = Starred Docs
dashboard-starred-docs-error = Failed to load starred docs
dashboard-starred-docs-empty-title = No starred pages
dashboard-starred-docs-empty-description = Star a doc to keep it handy.

dashboard-unassigned-queue-title = Unassigned Queue
dashboard-unassigned-queue-error = Failed to load queue
dashboard-unassigned-queue-empty-title = Inbox zero
dashboard-unassigned-queue-empty-description = Nothing waiting in the queue.

# Core UI layer
ui-site-header-untitled-ticket = Untitled Ticket
ui-site-header-unknown-device = Unknown Device
ui-site-header-ticket-title-placeholder = Enter ticket title...
ui-site-header-document-title-placeholder = Enter document title...
ui-site-header-create-aria = Create { $action }
ui-site-header-inbox-tooltip = Inbox
ui-site-header-inbox-aria = Open inbox
ui-user-selection-modal-title = Assign User
ui-user-selection-modal-search-placeholder = Search users by name or email...
ui-user-selection-modal-unassign = Unassign User
ui-user-selection-modal-error = Failed to load users
ui-user-selection-modal-empty-no-match = No users found
ui-user-selection-modal-empty-no-users = No users available
ui-user-selection-modal-role-admin = Admin
ui-user-selection-modal-role-technician = Agent
ui-user-selection-modal-role-user = User
ui-user-card-role-admin = Admin
ui-user-card-role-technician = Agent
ui-user-card-role-user = User
ui-quick-tooltip-unassigned = Unassigned
ui-quick-tooltip-status-label = Status:
ui-quick-tooltip-requester-label = Requester:
ui-quick-tooltip-assignee-label = Assignee:
ui-presence-stack-fallback-name = Someone
ui-presence-stack-aria =
    { $count ->
        [one] { $count } person viewing
       *[other] { $count } people viewing
    }
ui-presence-stack-overflow-title =
    { $count ->
        [one] { $count } more viewing
       *[other] { $count } more viewing
    }
ui-status-badge-status-open = open
ui-status-badge-status-in-progress = in-progress
ui-status-badge-status-closed = closed
ui-status-badge-priority-low = low
ui-status-badge-priority-medium = medium
ui-status-badge-priority-high = high
ui-status-badge-priority-low-full = low priority
ui-status-badge-priority-medium-full = medium priority
ui-status-badge-priority-high-full = high priority
ui-heatmap-tooltip-more = ...and { $count ->
        [one] { $count } more
       *[other] { $count } more
    }
ui-device-groups-title = Groups
ui-header-title-placeholder = Enter title...

# Search + ticket remnants
search-global-filter-documentation = Documentation
search-global-filter-tickets = Tickets
search-global-filter-devices = Assets
search-global-filter-users = Users
search-global-filter-projects = Projects
search-global-placeholder = Search tickets, docs, assets, users
search-global-placeholder-filtered = Search { $filter }
search-global-aria-label = Search
search-global-prompt-title = Search your helpdesk
search-global-prompt-subtitle = Find tickets, documentation, assets, and more
search-global-empty-prefix = No results for
search-global-empty-hint = Try different keywords or check your spelling
search-global-hint-navigate = Navigate
search-global-hint-open = Open
search-global-hint-close = Close
search-global-results-count =
    { $count ->
        [one] { $count } result
       *[other] { $count } results
    }
search-global-results-took = { $ms }ms
search-result-item-today = Today
search-result-item-yesterday = Yesterday
search-result-item-days-ago = { $count }d ago
search-result-item-weeks-ago = { $count }w ago
search-result-item-months-ago = { $count }mo ago
search-result-item-years-ago = { $count }y ago
search-result-item-internal-title = Internal note, visible to staff only
search-result-item-internal-badge = Internal
search-result-group-ticket = Tickets
search-result-group-comment = Comments
search-result-group-documentation = Documentation
search-result-group-attachment = Attachments
search-result-group-device = Assets
search-result-group-user = Users
tickets-cycle-burndown-load-error = Failed to load stats
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
tickets-cycle-burndown-days-remaining =
    { $count ->
        [one] Day remaining
       *[other] Days remaining
    }
tickets-cycle-burndown-snapshot-frozen = Snapshot frozen { $date }
tickets-cycle-burndown-carried-over =
    { $count ->
        [one] { $count } ticket carried over
       *[other] { $count } tickets carried over
    }
cycle-burnup-title = Burnup
cycle-burnup-legend-scope = Scope
cycle-burnup-legend-completed = Completed
cycle-burnup-legend-ideal = Ideal
cycle-burnup-legend-start-scope = Start scope
cycle-burnup-needs-dates = Add start and end dates to see the burnup.
tickets-cycle-scope-added = +{ $count } added after start
tickets-collaborative-article-title = Ticket Notes
tickets-collaborative-article-doc-title = Documentation: Ticket #{ $id }
tickets-collaborative-article-revision-history = Revision history
tickets-collaborative-article-convert-doc = Convert to documentation page
tickets-collaborative-article-open-full = Open full editor
tickets-project-info-remove = Remove from project
tickets-project-info-description = Description
tickets-project-info-project-id = Project ID
tickets-project-info-status = Status
tickets-project-info-tickets = Tickets
tickets-project-info-print-tickets =
    { $count ->
        [one] { $count } ticket
       *[other] { $count } tickets
    }
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
header-create-ticket = New ticket
header-create-project = Create project
header-add-ticket = Add ticket
header-create-user = Create user
header-create-asset = Create asset
header-create-document = Create document
nav-route-announcement = Navigated to { $title }
common-dropdown-select-placeholder = Select an option
common-dropdown-empty-message = No matches

# Inbox + notifications polish
inbox-group-today = Today
inbox-group-yesterday = Yesterday
inbox-group-this-week = This week
inbox-group-earlier = Earlier
inbox-mark-mentions-read = Mark mentions as read
inbox-mark-all-read = Mark all as read
inbox-empty-caught-up-title = You're all caught up
inbox-empty-caught-up-subtitle = Nothing unread right now. New notifications will appear here as they arrive.
inbox-empty-mentions-title = No mentions yet
inbox-empty-mentions-subtitle = When someone @mentions you in a comment, you'll see it here.
inbox-empty-default-title = No notifications yet
inbox-empty-default-subtitle = Updates from tickets, comments, mentions, and docs you follow will land here.
inbox-footer-loading-more = Loading more...
inbox-footer-end-of-feed = End of feed
notifications-bell-header = Notifications
notifications-bell-open-inbox = Open inbox
notifications-bell-loading = Loading...
notifications-bell-load-more = Load more
notifications-bell-mark-mentions-read = Mark mentions as read
notifications-bell-mark-all-read = Mark all as read
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
route-title-asset-create = Create asset
route-title-asset-view = Asset details
route-title-asset-planner = Asset planner
route-title-project-detail = Project details
route-title-error = Error
route-title-users = Users
route-title-documentation-drafts = Drafts
route-title-collection = Collection
route-title-documentation-archived = Archived
route-title-documentation-trash = Trash
route-title-knowledge-gaps = Knowledge gaps
route-title-profile = Profile
route-title-profile-settings = Settings
route-title-profile-settings-profile = Profile settings
route-title-profile-settings-appearance = Appearance settings
route-title-profile-settings-notifications = Notification settings
route-title-profile-settings-security = Security settings
route-title-administration = Administration
route-title-admin-groups = Groups
route-title-group-configuration = Group configuration
route-title-admin-categories = Categories
route-title-admin-assignment-rules = Assignment rules
route-title-admin-workflow = Workflow
route-title-admin-asset-kinds = Asset kinds
route-title-admin-asset-kinds-new = New asset kind
route-title-admin-asset-kinds-edit = Edit asset kind
route-title-admin-api-tokens = API tokens
route-title-admin-workspaces = Workspaces
route-title-admin-workspace-members = Workspace members
route-title-admin-canned-responses = Canned responses
route-title-admin-canned-responses-new = New canned response
route-title-admin-canned-responses-edit = Edit canned response
route-title-admin-webhooks = Webhooks
route-title-admin-sla = SLA
route-title-admin-plugins = Plugins
route-title-admin-plugin-registry = Plugin registry
route-title-admin-plugin-sideload = Sideload plugin
route-title-admin-plugin-detail = Plugin detail
route-title-admin-auth-providers = Authentication providers
route-title-admin-search = Search index management
route-title-admin-system-settings = System settings
route-title-admin-branding = Branding
route-title-admin-audit-log = Audit log
route-title-admin-email-queue = Email queue
route-title-admin-email-suppressions = Email suppressions
route-title-admin-guest-access = Guest access
route-title-admin-email-settings = Email configuration
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
common-loading-devices = Loading assets...
common-loading-generic = Loading...
common-loading-groups = Loading groups...
common-delete-item-aria = Delete { $name }
admin-branding-aria-logo = Logo
admin-branding-aria-logo-light = Light theme logo
admin-branding-aria-favicon = Favicon

# Store error messages
# Shown to the user via toast or inline error banners when a Pinia
# store action fails. Keep these short; they appear alongside the
# raw error.message when one is available.
error-store-workflow-states-load = Failed to load workflow states.
error-store-public-settings-load = Failed to load public settings.
error-store-feature-flags-load = Failed to load feature flags.
error-store-recent-tickets-load = Failed to fetch recent tickets.
error-store-saved-views-load = Failed to load saved views.
error-store-saved-view-save = Failed to save view.
error-store-saved-view-update = Failed to update view.
error-store-saved-view-delete = Failed to delete view.
error-store-cycles-load = Failed to load cycles.
error-store-cycle-create = Failed to create cycle.
error-store-cycle-update = Failed to update cycle.
error-store-cycle-complete = Failed to complete cycle.
error-store-cycle-archive = Failed to archive cycle.
error-store-auth-profile-load = Failed to load profile data. Please try again.
error-store-auth-mfa-setup-start = Failed to start MFA setup. Please try again.
error-store-auth-mfa-setup-complete = Failed to complete MFA setup. Please try again.

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
status-in-progress = In Progress
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
search-entity-type-devices = Assets
search-entity-type-users = Users
search-entity-type-projects = Projects
plugin-permission-ticket-read-label = Read Tickets
plugin-permission-ticket-read-description = Read ticket data
plugin-permission-ticket-write-label = Write Tickets
plugin-permission-ticket-write-description = Create and update tickets
plugin-permission-ticket-comment-label = Comment on Tickets
plugin-permission-ticket-comment-description = Add comments to tickets
plugin-permission-ticket-delete-label = Delete Tickets
plugin-permission-ticket-delete-description = Delete tickets
plugin-permission-asset-read-label = Read assets
plugin-permission-asset-read-description = Read asset data
plugin-permission-asset-write-label = Write assets
plugin-permission-asset-write-description = Create and update assets
plugin-permission-user-read-label = Read Users
plugin-permission-user-read-description = Read user profile data
plugin-permission-storage-plugin-label = Plugin Storage
plugin-permission-storage-plugin-description = Store plugin-scoped key-value data
plugin-permission-collection-read-label = Read Collections
plugin-permission-collection-read-description = Read typed collection rows
plugin-permission-collection-write-label = Write Collections
plugin-permission-collection-write-description = Create and update typed collection rows
error-resource-not-found = The requested resource was not found.
error-network = Unable to connect to the server. Please check your internet connection.
error-session-expired = Your session has expired. Please log in again.
error-forbidden = You do not have permission to perform this action.
plugin-error-load-failed = Failed to load plugin
plugin-error-pending-review = This plugin is pending review
plugin-error-not-installed = Plugin component not installed
plugin-error-component-not-found = Component not found in plugin
plugin-error-timeout = Plugin took too long to load
plugin-error-failed = Plugin failed to load
bulk-action-undo = Undo
bulk-action-undone = Undone
bulk-action-undo-failed = Undo failed
passkey-last-used-never = Never

# Dashboard widget registry
dashboard-widget-assigned-tickets-title = Assigned tickets
dashboard-widget-assigned-tickets-description = Your current work queue with status and priority.
# Replaced by the system-* keys below; kept temporarily in case any
# user has the legacy widget id pinned to a custom layout. v1.1
# removes these after the layout migrator drops the stale ids.
dashboard-widget-stats-yours-title = Your counts
dashboard-widget-stats-yours-description = Quick counts of tickets assigned to you by status.
dashboard-widget-stats-queue-title = Queue counts
dashboard-widget-stats-queue-description = Unassigned and total ticket counts across the queue.

# System dashboard widgets (the chart-backed defaults). Titles are
# shown both in the widget header on the canvas and in the Add
# Widget picker; descriptions only appear in the picker.
dashboard-system-tickets-created-title = Created
dashboard-system-tickets-created-description = Tickets created in the current time range.
dashboard-system-tickets-resolved-title = Resolved
dashboard-system-tickets-resolved-description = Tickets resolved in the current time range.
dashboard-system-tickets-open-title = Open
dashboard-system-tickets-open-description = Tickets currently in an open state.
dashboard-system-tickets-over-time-title = Tickets over time
dashboard-system-tickets-over-time-description = Daily inflow line, scaled to the active time range.
dashboard-system-volume-by-category-title = Volume by category
dashboard-system-volume-by-category-description = Top categories by ticket count for the active window.
dashboard-system-volume-by-priority-title = Volume by priority
dashboard-system-volume-by-priority-description = Ticket counts broken down by priority bucket.
dashboard-widget-unassigned-queue-title = Unassigned queue
dashboard-widget-unassigned-queue-description = Oldest open tickets with no assignee. Grab the next one.
dashboard-widget-recently-viewed-title = Recently viewed
dashboard-widget-recently-viewed-description = Tickets you looked at most recently.
dashboard-widget-starred-docs-title = Starred docs
dashboard-widget-starred-docs-description = Documentation pages you have starred for quick access.
dashboard-widget-my-devices-title = My assets
dashboard-widget-my-devices-description = Assets assigned to you as their primary user.
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

# SLA workspace health: at-a-glance breakdown of currently tracked
# tickets across all policies. Refreshes every 30s.
dashboard-widget-sla-health-title = SLA health
dashboard-widget-sla-health-description = Workspace-wide breakdown of tickets covered by an SLA policy.
dashboard-sla-health-title = SLA health
dashboard-sla-health-action = SLA admin
dashboard-sla-health-tracked = Tracked
dashboard-sla-health-breached = Breached
dashboard-sla-health-at-risk = At risk
dashboard-sla-health-paused = Paused
dashboard-sla-health-error = Couldn't load SLA health
dashboard-sla-health-empty-title = No tickets tracked
dashboard-sla-health-empty-description = No open tickets currently match any SLA policy.
# Q1 polish: template attributes + static text
tickets-row-new-activity-tooltip = New activity since you last viewed this
tickets-row-new-activity-aria = New activity
common-confirm-delete-title = Confirm Delete
common-toast-dismiss = Dismiss
common-error-banner-dismiss = Dismiss error
common-route-progress-aria = Loading
common-bulk-actions-aria = Bulk actions
common-loading-more-aria = Loading more
pagination-controls-page = Page
pagination-controls-show = Show
pagination-controls-id-placeholder = ID
asset-modal-title = Select an asset
asset-modal-search-placeholder = Search assets by name, hostname, serial number, manufacturer, or user...
asset-modal-owner = Owner
asset-modal-unassigned = Unassigned
asset-modal-col-device = Asset
asset-modal-col-status = Status
asset-modal-col-serial = Serial
asset-modal-col-user = User
project-modal-title = Add to Project
project-modal-search-placeholder = Search projects by name or description...
project-modal-col-name = Project Name
project-modal-col-description = Description
project-modal-col-status = Status
project-modal-col-tickets = Tickets
project-modal-col-action = Action
kanban-recurring-tooltip = Recurring ticket
kanban-recurring-aria = Recurring
kanban-sla-aria = SLA status
kanban-quick-add-placeholder = New ticket title...
kanban-quick-add-aria = Add ticket to { $column }
calendar-today = Today
calendar-anchor-label = Anchor
calendar-anchor-tooltip = Anchor field is set by the saved view; future commits surface the picker.
calendar-anchor-due-date = Due date
calendar-anchor-created = Created
calendar-anchor-last-activity = Last activity
gantt-today = Today
gantt-title = Gantt
gantt-zoom-week = Week
gantt-zoom-month = Month
gantt-zoom-quarter = Quarter
gantt-fit = Fit
gantt-pan-previous = Pan earlier
gantt-pan-next = Pan later
gantt-unscheduled = Unscheduled ({ $count })
gantt-empty-window = No tickets fall inside this window. Use Fit, Today, or the pan arrows to step the timeline.
gantt-reschedule-handle = Drag to change due date
user-cell-missing-tooltip = This user no longer exists
user-cell-unknown = Unknown
user-settings-managing-for = Managing settings for
user-settings-groups-title = Groups
user-settings-role-management-title = Role Management
user-settings-account-setup-title = Account Setup
user-settings-account-setup-pending = Pending
user-settings-invitation-pending = Invitation pending
user-settings-resend-invitation-title = Resend Invitation Email
user-settings-danger-zone-title = Danger Zone
user-settings-danger-zone-subtitle = Irreversible and destructive actions
user-settings-delete-modal-title = Confirm Account Deletion
user-settings-delete-item-profile = Profile information and settings
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

# R1 auth UX errors
auth-mfa-check-failed = Failed to check MFA status
auth-mfa-setup-failed = Failed to setup MFA
auth-mfa-setup-failed-retry = MFA setup failed. Please try again.
auth-mfa-code-invalid = Please enter a valid 6-digit code
auth-mfa-secret-missing = MFA secret is missing. Please restart the setup process.
auth-mfa-verify-failed = Invalid verification code. Please try again.
auth-mfa-enable-failed = Failed to enable MFA
auth-mfa-disable-failed = Failed to disable MFA
auth-mfa-backup-codes-failed = Failed to regenerate backup codes
auth-passkey-load-failed = Failed to load passkeys
auth-passkey-not-supported-browser = Passkeys are not supported in this browser
auth-passkey-not-supported-device = Passkeys are not supported on this device
auth-passkey-max-reached = Maximum number of passkeys reached (10)
auth-passkey-registered = Passkey "{ $name }" registered successfully
auth-passkey-registration-not-allowed = Registration was cancelled or not allowed
auth-passkey-already-registered = This passkey is already registered
auth-passkey-registration-cancelled = Registration was cancelled
auth-passkey-register-failed = Failed to register passkey
auth-passkey-login-success = Logged in successfully with passkey
auth-passkey-auth-not-allowed = Authentication was cancelled or not allowed
auth-passkey-none-registered = No passkeys registered for this account
auth-passkey-auth-cancelled = Authentication was cancelled
auth-passkey-login-failed = Failed to login with passkey
auth-passkey-name-required = Passkey name is required
auth-passkey-name-too-long = Passkey name must be 100 characters or less
auth-passkey-renamed = Passkey renamed successfully
auth-passkey-rename-failed = Failed to rename passkey
auth-passkey-delete-password-required = Password is required to delete a passkey
auth-passkey-deleted = Passkey deleted successfully
auth-passkey-incorrect-password = Incorrect password
auth-passkey-delete-failed = Failed to delete passkey
ticket-data-load-failed = Failed to load ticket. Please try again later.
plugins-load-failed = Failed to load plugins
search-failed = Search failed. Please try again.
auth-autologin-prompt = Please log in with your credentials.
auth-login-rate-limited = Too many requests. Please wait a moment.
auth-mfa-rate-limited = Too many MFA attempts. Please try again later.
auth-mfa-rate-limited-retry = Too many MFA attempts. Please try again in { $seconds } seconds.
auth-mfa-failed = MFA verification failed. Please try again.
auth-login-network-error = Network error. Please check your connection.
auth-login-backup-codes-low = Login successful! Please regenerate your backup codes soon, you have 2 or fewer remaining.
ticket-audio-play-failed = Failed to play audio
asset-modal-load-failed = Failed to load assets. Please try again.
project-modal-load-failed = Failed to load projects. Please try again later.
user-profile-load-failed = Failed to load user information
# R2 filter facets + ticket columns
filter-facet-title = Title
filter-facet-status = Status
filter-facet-priority = Priority
filter-facet-assignee = Assignee
filter-facet-sla = SLA
filter-facet-cycle = Cycle
filter-assignee-unassigned = Unassigned
filter-assignee-loading = Loading…
filter-cycle-option = Cycle #{ $id }
filter-summary-n-selected = { $count } selected
tickets-column-id = #
tickets-column-id-description = Ticket number
tickets-column-title = Title
tickets-column-title-description = Ticket subject
tickets-column-status = Status
tickets-column-status-description = Workflow state
tickets-column-priority = Priority
tickets-column-priority-description = Priority
tickets-column-assignee = Assignee
tickets-column-assignee-description = Who owns the ticket
tickets-column-requester = Requester
tickets-column-requester-description = Who reported the ticket
tickets-column-category = Category
tickets-column-category-description = Ticket category tag
tickets-column-cycle = Cycle
tickets-column-cycle-description = Cycle membership
tickets-column-due-date = Due
tickets-column-due-date-description = Calendar deadline
tickets-column-last-activity = Updated
tickets-column-last-activity-description = When the ticket last changed
tickets-column-created-at = Created
tickets-column-created-at-description = When the ticket was opened
tickets-column-sla = SLA
tickets-column-sla-description = SLA pill (green / amber / red)
tickets-column-kb-gap = KB
tickets-column-kb-gap-description = Knowledge-gap signal
tickets-column-devices = Assets
tickets-column-devices-description = Affected asset count
tickets-column-recurrence = Recur
tickets-column-recurrence-description = Recurring ticket marker

# Backend HTTP error responses (R3)
backend-error-auth-required = Authentication required.
backend-error-user-not-found = User account not found.
backend-error-comment-fetch-failed = Couldn't load comments.
backend-error-comment-create-failed = Couldn't create the comment.
backend-error-comment-not-found = Comment not found.
backend-error-attachment-not-found = Attachment not found.
backend-error-attachment-delete-failed = Couldn't delete the attachment.

# S2 backend handler error sweep
backend-error-validation = Validation error.
backend-error-passkey-max-reached = Maximum number of passkeys reached.
backend-error-bad-request = Bad request.
backend-error-search-failed = Search failed.
backend-error-search-rebuild-failed = Index rebuild failed.

# S1 frontend registries + defaults
builtin-view-my-open-name = My Open
builtin-view-my-open-description = Open tickets assigned to you
builtin-view-my-active-name = My Active
builtin-view-my-active-description = Unresolved tickets assigned to you
builtin-view-all-active-name = All Active
builtin-view-all-active-description = Every ticket that hasn't been resolved or cancelled
builtin-view-all-tickets-name = All Tickets
builtin-view-all-tickets-description = Every ticket, including resolved and cancelled
builtin-view-unassigned-name = Unassigned
builtin-view-unassigned-description = Active tickets with no assignee
builtin-view-overdue-name = Overdue
builtin-view-overdue-description = Active tickets past their due date
builtin-view-triage-name = Triage
builtin-view-triage-description = Tickets awaiting initial categorization
builtin-view-calendar-name = Calendar
builtin-view-calendar-description = Tickets placed on the day they are due
workflow-category-triage = Triage
workflow-category-backlog = Backlog
workflow-category-active = Active
workflow-category-in-review = In Review
workflow-category-done = Done
workflow-category-cancelled = Cancelled
assignment-method-direct-user-name = Direct User
assignment-method-direct-user-description = Assign directly to a specific user
assignment-method-group-round-robin-name = Round-Robin (Group)
assignment-method-group-round-robin-description = Rotate assignment among group members evenly
assignment-method-group-random-name = Random (Group)
assignment-method-group-random-description = Randomly select a group member for each ticket
assignment-method-group-queue-name = Group Queue
assignment-method-group-queue-description = Assign to group queue (users claim tickets)
tickets-category-none = No category
tickets-menu-flag-for-docs = Flag for documentation
tickets-menu-delete = Delete ticket
docs-author-system = System
docs-untitled-page = Untitled
profile-role-user-label = User
profile-role-user-description = Can create tickets and view assigned resources
profile-role-technician-label = Agent
profile-role-technician-description = Can manage tickets, assets, and assist other users
profile-role-admin-label = Administrator
profile-role-admin-description = Full access to all system features and user management
tickets-grouping-no-cycle = No cycle
list-grouping-none = No grouping
list-grouping-trigger = Group by
views-column-picker-trigger = Columns
views-column-picker-reset = Reset columns
views-column-resize-handle-tooltip = Drag to resize
assets-list-grouping-warranty = Warranty
assets-list-grouping-kind = Type
assets-list-grouping-manufacturer = Manufacturer
assets-list-grouping-manufacturer-none = No manufacturer
assets-list-grouping-location = Location
assets-list-grouping-location-none = No location
assets-list-grouping-primary-user = Primary user
tickets-grouping-all = All

# T batch: final sweep
error-api-server = A server error occurred. Please try again later.
error-api-validation = The provided data is invalid.
error-api-generic = An error occurred while processing your request.
plugin-loader-error = Couldn't load plugins.
seed-welcome-page-title = Welcome to Nosdesk
email-notice-security = Security Notice
email-notice-security-critical = Critical Security Notice
email-notice-getting-started = Getting Started
email-notice-success = Success
email-link-fallback-prompt = Or copy and paste this link into your browser:
email-footer-rights = All rights reserved.
email-footer-automated = This is an automated message. Please do not reply directly to this email.
markdown-embed-depth-limit = [Embed depth limit reached]
markdown-embed-circular = [Circular embed detected]
markdown-embed-reference = Embedded: { $title }
markdown-embed-reference-fallback = [Embedded: { $title }]

# V batch: editor plugins + SSE
editor-embed-empty-document = Empty document
editor-embed-load-failed = Couldn't load document
editor-embed-open-document = Open document
editor-loading = Loading...
sse-connection-failed = Connection failed.
sse-no-auth-token = Not signed in.
auth-microsoft-logout-failed = Couldn't sign out of Microsoft.
editor-ticket-link-not-found = Ticket #{ $id } not found

# W batch: pluralization fixes
notifications-inbox-unread-count =
    { $count ->
        [one] { $count } unread notification
       *[other] { $count } unread notifications
    }
gantt-tickets-in-view =
    { $count ->
        [one] { $count } ticket in view
       *[other] { $count } tickets in view
    }
bulk-bar-select-all-matching = Select all { $count }
bulk-bar-clear = Clear
bulk-bar-selected-generic =
    { $count ->
        [one] { $count } selected
       *[other] { $count } selected
    }
bulk-bar-all-selected-generic = All { $count } selected
bulk-bar-tickets-selected =
    { $count ->
        [one] { $count } ticket selected
       *[other] { $count } tickets selected
    }
bulk-bar-tickets-all-selected = All { $count } tickets selected
bulk-bar-users-selected =
    { $count ->
        [one] { $count } user selected
       *[other] { $count } users selected
    }
bulk-bar-users-all-selected = All { $count } users selected
bulk-bar-devices-selected =
    { $count ->
        [one] { $count } device selected
       *[other] { $count } devices selected
    }
bulk-bar-devices-all-selected = All { $count } assets selected
inbox-no-unread = You have no unread notifications.
gantt-tickets-of-total-in-view =
    { $count ->
        [one] { $visible } of { $count } ticket in view
       *[other] { $visible } of { $count } tickets in view
    }
saved-view-name-this = Name this view
saved-view-copy-suffix = { $name } copy

# Rules engine (docs/rules-and-actions-plan.md). Admin Settings →
# Rules surfaces every rule kind; the agent toolbar surfaces only
# the manual subset under the label "Actions". Activity log /
# inspector wording stays consistent with the audit framing
# established by canned-responses and ticket-merge.

route-title-admin-rules = Rules
route-title-admin-rules-activity = Rule activity
route-title-admin-rules-new = New rule
route-title-admin-rules-edit = Edit rule

admin-rules-title = Rules
admin-rules-help-intro = One rule entity covers manual quick-actions, on-event automations, and time-based escalations. Manual rules show up in the agent toolbar; everything else fires from the engine.
admin-rules-new-cta = New rule
admin-rules-activity-cta = Recent activity
admin-rules-search-placeholder = Search rules by name
admin-rules-filter-trigger-all = All triggers
admin-rules-filter-state-all = All states
admin-rules-trigger-manual = Manual
admin-rules-trigger-ticket-created = On ticket created
admin-rules-trigger-ticket-updated = On ticket updated
admin-rules-trigger-ticket-replied = On reply
admin-rules-trigger-time-elapsed = Time elapsed
admin-rules-state-draft = Draft
admin-rules-state-dry-run = Dry run
admin-rules-state-live = Live
admin-rules-state-archived = Archived
admin-rules-col-name = Name
admin-rules-col-trigger = Trigger
admin-rules-col-state = State
admin-rules-col-last-fired = Last fired
admin-rules-col-fire-count = Fires (total)
admin-rules-last-fired-never = Never
admin-rules-empty-title = No rules yet
admin-rules-empty-hint = Create your first rule or browse the starter catalog (coming soon).
admin-rules-error-load = Couldn't load the rules list.
admin-rules-error-archive = Couldn't archive that rule.
admin-rules-error-transition = Couldn't change the state.
admin-rules-toast-archived = Archived { $name }.
admin-rules-toast-state-changed = State changed to { $state }.
admin-rules-action-pause-tooltip = Pause (move to dry run)
admin-rules-action-resume-tooltip = Resume (back to live)
admin-rules-action-archive-tooltip = Archive
admin-rules-archive-confirm-title = Archive rule?
admin-rules-archive-confirm-body = { $name } will stop firing and be hidden from the picker. The audit history stays. You can permanently delete it later from the archived view.
admin-rules-archive-confirm-button = Archive

admin-rules-activity-title = Rule activity
admin-rules-activity-help = Every fire writes one row, whether successful, skipped, suppressed, or failed. Click a row to see the condition evaluation and the actions that ran.
admin-rules-activity-back = Back to rules
admin-rules-activity-error-load = Couldn't load the activity log.
admin-rules-activity-empty-title = No activity yet
admin-rules-activity-empty-hint = Rules write here as soon as they fire.
admin-rules-activity-filter-all = All statuses
admin-rules-activity-limit = Last { $n }
admin-rules-activity-actor-system = engine
admin-rules-activity-actor-user = agent
admin-rules-activity-row-summary = on ticket #{ $ticket_id } by { $actor }
admin-rules-activity-inspector-empty = No inspector payload (successful fire keeps the audit row tight).
admin-rules-activity-status-succeeded = Succeeded
admin-rules-activity-status-dry-run = Dry run
admin-rules-activity-status-skipped-preflight = Skipped (preflight)
admin-rules-activity-status-skipped-condition-unmet = Skipped (no match)
admin-rules-activity-status-suppressed-recursion-budget = Suppressed (recursion)
admin-rules-activity-status-suppressed-loop-guard = Suppressed (loop guard)
admin-rules-activity-status-failed = Failed

# Activity-feed phrases for the rule fire event. Wave 7 wires
# ticket.rule_applied into TicketActivity.vue; the dry-run variant
# distinguishes shadow fires (state = dry_run) from live ones.
ticket-activity-phrase-rule-applied = applied rule "{ $rule }"
ticket-activity-phrase-rule-applied-dry-run = previewed rule "{ $rule }" in dry-run

# Actions toolbar (agent surface, decision 26: button label is
# "Actions" even though the backend entity is Rule).
ticket-actions-button = Actions
ticket-actions-dialog-title = Apply an action
ticket-actions-dialog-picker-placeholder = Find an action...
ticket-actions-dialog-empty = No live manual rules in this workspace.
ticket-actions-dialog-action-list-label = This action will:
ticket-actions-dialog-cancel = Cancel
ticket-actions-dialog-apply = Apply
ticket-actions-dialog-applying = Applying...
ticket-actions-success-toast = Applied "{ $rule }".
ticket-actions-error-toast = Couldn't apply the action.

# Action summary chips (admin list + agent dialog preview).
admin-rules-action-chip-reply-public = Reply to customer
admin-rules-action-chip-reply-internal = Add internal note
admin-rules-action-chip-set-status = Move to state #{ $state_id }
admin-rules-action-chip-assign = Assign to user
admin-rules-action-chip-unassign = Clear assignee
admin-rules-action-chip-add-tags = Add { $count ->
    [one] 1 tag
   *[other] { $count } tags
  }
admin-rules-action-chip-remove-tags = Remove { $count ->
    [one] 1 tag
   *[other] { $count } tags
  }
admin-rules-action-chip-set-priority = Set priority to { $priority }
admin-rules-action-chip-notify = Send notification
admin-rules-action-chip-stop-processing = Stop here

# Rule editor (Wave 7).
admin-rule-editor-title-new = New rule
admin-rule-editor-title-edit = Edit "{ $name }"
admin-rule-editor-back = Back to rules
admin-rule-editor-save = Save
admin-rule-editor-saving = Saving...
admin-rule-editor-section-name = What
admin-rule-editor-section-trigger = When
admin-rule-editor-section-actions = Then do
admin-rule-editor-section-state = State
admin-rule-editor-name-label = Name
admin-rule-editor-name-placeholder = Acknowledge and escalate to network team
admin-rule-editor-description-label = Description (optional)
admin-rule-editor-description-placeholder = Short note about when an agent should reach for this.
admin-rule-editor-trigger-label = Trigger
admin-rule-editor-trigger-manual-note = Manual rules show up in the agent Actions toolbar. There's no per-ticket condition; the picker is filtered by category.
admin-rule-editor-trigger-other-phase = Event triggers and time-elapsed triggers land in Phase 2 of the rules engine; for now they save as Draft but won't fire until the engine subscribes.
admin-rule-editor-actions-add = Add an action
admin-rule-editor-actions-empty = This rule needs at least one action.
admin-rule-editor-action-remove = Remove
admin-rule-editor-error-save = Couldn't save the rule.
admin-rule-editor-error-conflict = This rule reads and writes the same fields. Save anyway to override.
admin-rule-editor-override-self-ref = I understand this rule may loop
admin-rule-editor-priority-label = Priority (lower runs first)

# Dashboard chrome row (docs/dashboard-and-analytics-plan.md Wave 1).
# Time-range chip cluster, compare-to-prior toggle, audit-log
# annotation overlay toggle, R-refresh button with "Updated X ago"
# indicator. Seven section anchor labels live alongside; the
# AnchorRail component reads them when Wave 8 wires it in.

dashboard-time-range-today = Today
dashboard-time-range-7d = 7d
dashboard-time-range-30d = 30d
dashboard-time-range-90d = 90d
dashboard-time-range-1y = 1y
dashboard-time-range-3y = 3y
dashboard-time-range-custom = Custom
dashboard-time-range-custom-from = From
dashboard-time-range-custom-to = To
dashboard-time-range-custom-apply = Apply
dashboard-time-range-custom-cancel = Cancel

dashboard-compare-toggle-label = Compare
dashboard-compare-toggle-tooltip = Overlay the same range from the prior period

dashboard-annotations-toggle-label = Annotations
dashboard-annotations-toggle-tooltip = Mark rule, SLA, and business-hours edits on time-series charts

dashboard-refresh-tooltip = Refresh non-live data (R)
dashboard-refresh-updated-prefix = Updated
dashboard-refresh-just-loaded = just now
dashboard-refresh-unknown = recently

dashboard-anchor-rail-aria-label = Dashboard sections
dashboard-section-today = Today
dashboard-section-volume-sla = Volume & SLA
dashboard-section-queue-health = Queue Health
dashboard-section-agents = Agents
dashboard-section-categories = Categories
dashboard-section-backlog-ageing = Backlog & Ageing
dashboard-section-audit-activity = Audit Activity
