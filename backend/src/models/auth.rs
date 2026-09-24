use super::users::UserResponse;
use diesel::deserialize::{self, FromSql};
use diesel::pg::{Pg, PgValue};
use diesel::prelude::*;
use diesel::serialize::{self, IsNull, Output, ToSql};
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

// JWT Claims structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,   // Subject (user UUID as string for JWT compatibility)
    pub name: String,  // User's name
    pub email: String, // User's email
    /// Platform-wide privilege role: `"platform_admin"` /
    /// `"audit_reviewer"` / `"user"`. The per-workspace role is NOT
    /// carried in the token (the JWT stays workspace-independent); it
    /// is resolved per-request from `workspace_members`. Defaulted on
    /// deserialize so a stray pre-W2 token without the claim degrades
    /// to a plain user (and is re-minted with the real value on the
    /// next 15-minute refresh) rather than failing to parse.
    #[serde(default = "default_platform_role")]
    pub platform_role: String,
    #[serde(default = "default_scope")]
    // Default to "full" for backward compatibility with existing tokens
    pub scope: String, // Token scope: "full" for normal sessions
    #[serde(default)] // Session ID (UUID) — None for SSE/API tokens
    pub sid: Option<String>,
    /// Workspace selected when an SSE token was minted (Model C). EventSource
    /// can't send the `X-Nosdesk-Workspace` header, so the selected workspace
    /// is bound into the SSE token instead and the stream authorizes against
    /// it. `None` on session/API tokens (which resolve the workspace per
    /// request) and on SSE tokens minted before this claim existed (the stream
    /// falls back to the Host-derived context).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_uuid: Option<Uuid>,
    pub exp: usize, // Expiration time
    pub iat: usize, // Issued at
}

impl Claims {
    /// Parse the `sid` claim into a UUID. Returns None for SSE/API tokens.
    pub fn session_uuid(&self) -> Option<Uuid> {
        self.sid.as_deref().and_then(|s| s.parse().ok())
    }
}

// Default scope for backward compatibility
fn default_scope() -> String {
    "full".to_string()
}

// Default platform role for tokens minted before the claim existed.
fn default_platform_role() -> String {
    "user".to_string()
}

// Login request structure
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

// Login response structure - supports both standard login and MFA flow
// Note: tokens are now in httpOnly cookies, only CSRF token is in response body
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub mfa_required: Option<bool>,
    pub mfa_setup_required: Option<bool>,
    pub passkey_mfa_required: Option<bool>,
    pub user_uuid: Option<String>,
    pub csrf_token: Option<String>, // CSRF token for the frontend
    pub user: Option<UserResponse>,
    pub message: Option<String>,
    pub mfa_backup_code_used: Option<bool>,
    pub requires_backup_code_regeneration: Option<bool>,
    pub backup_codes: Option<Vec<String>>, // Present when MFA is enabled during login setup
    // Native/bearer clients (X-Auth-Mode: bearer) receive the session tokens in
    // the body instead of httpOnly cookies. Omitted entirely for web clients, so
    // the cookie-flow JSON is byte-identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

/// Request for MFA verification during login
#[derive(Debug, Deserialize)]
pub struct MfaLoginRequest {
    pub email: String,
    pub password: String,
    pub mfa_token: String,
}

/// Request for recovery code login (passkey-MFA users who can't use their passkey)
#[derive(Debug, Deserialize)]
pub struct RecoveryLoginRequest {
    pub email: String,
    pub password: String,
    pub recovery_code: String,
}

/// Request for MFA setup during login (unauthenticated)
#[derive(Debug, Deserialize)]
pub struct MfaSetupLoginRequest {
    pub email: String,
    pub password: String,
}

/// Request for enabling MFA during login (unauthenticated).
///
/// The TOTP secret is intentionally NOT in this struct: the matching
/// `mfa_setup_login` call stashes it server-side and the enable
/// handler retrieves it from there. Accepting it from the client
/// would let an attacker who knew the victim's password substitute
/// their own attacker-controlled secret + code and enroll their
/// authenticator on the victim's account.
#[derive(Debug, Deserialize)]
pub struct MfaEnableLoginRequest {
    pub email: String,
    pub password: String,
    pub token: String,
}

/// Response for token refresh
/// Web clients get rotated tokens in httpOnly cookies (only CSRF is in the body).
/// Native/bearer clients get the rotated session tokens in the body instead.
#[derive(Debug, Serialize)]
pub struct RefreshTokenResponse {
    pub success: bool,
    pub csrf_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

/// Optional body for `POST /api/auth/refresh`. Native/bearer clients send the
/// rotating refresh token here (web clients send it in the httpOnly cookie).
#[derive(Debug, Deserialize, Default)]
pub struct RefreshRequest {
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct PasswordChangeRequest {
    pub current_password: String,
    pub new_password: String,
}

// Authentication Provider models
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
pub enum AuthProviderType {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "microsoft")]
    Microsoft,
    #[serde(rename = "google")]
    Google,
    #[serde(rename = "saml")]
    Saml,
}

impl ToSql<diesel::sql_types::Text, Pg> for AuthProviderType {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            AuthProviderType::Local => "local",
            AuthProviderType::Microsoft => "microsoft",
            AuthProviderType::Google => "google",
            AuthProviderType::Saml => "saml",
        };
        out.write_all(s.as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<diesel::sql_types::Text, Pg> for AuthProviderType {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"local" => Ok(AuthProviderType::Local),
            b"microsoft" => Ok(AuthProviderType::Microsoft),
            b"google" => Ok(AuthProviderType::Google),
            b"saml" => Ok(AuthProviderType::Saml),
            _ => Err("Unrecognized auth provider type".into()),
        }
    }
}

// Environment-based AuthProvider struct (replaces database-stored providers)
#[derive(Debug, Clone)]
pub struct AuthProvider {
    pub id: i32,
    pub name: String,
    pub provider_type: String,
    pub enabled: bool,
    pub is_default: bool,
}

impl AuthProvider {
    pub fn new(
        id: i32,
        name: String,
        provider_type: String,
        enabled: bool,
        is_default: bool,
    ) -> Self {
        Self {
            id,
            name,
            provider_type,
            enabled,
            is_default,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigItem {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthProviderConfigResponse {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

// OAuth state management
#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthState {
    pub state: String,
    pub redirect_uri: String,
    pub provider_type: String,
    pub exp: usize,
    pub user_connection: Option<bool>,
    /// PKCE code verifier (for OIDC providers)
    pub pkce_verifier: Option<String>,
    /// Nonce for ID token validation (for OIDC providers)
    pub nonce: Option<String>,
    /// OAuth `redirect_uri` (the IdP callback) used for THIS flow, bound
    /// at initiation so the token exchange presents the identical value.
    /// In hosted mode each tenant authenticates on its own subdomain, so
    /// this is `https://<tenant-host>/api/auth/oauth/callback`, derived
    /// from the initiating request's `Host`. `None` means "use the
    /// statically configured `OIDC_REDIRECT_URI`" (self-hosted, and legacy
    /// in-flight tokens minted before this field existed).
    #[serde(default)]
    pub callback_redirect_uri: Option<String>,
    /// Per-flow random value bound to the initiating user-agent via the
    /// `oauth_state` cookie (RFC 9700 §2.1). The callback rejects unless the
    /// cookie matches this value, so an attacker can't CSRF their own
    /// `(code, state)` onto a victim (login-CSRF / session swap). `None` for
    /// legacy in-flight tokens minted before this field existed (a <=10-minute
    /// transition window, after which every state carries a binding).
    #[serde(default)]
    pub binding: Option<String>,
}

// OAuth Authentication request
#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthRequest {
    pub provider_type: String,
    pub redirect_uri: Option<String>,
    pub user_connection: Option<bool>,
}

// OAuth callback/exchange parameters
#[derive(Debug, Deserialize)]
pub struct OAuthExchangeRequest {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

// Onboarding models
#[derive(Debug, Serialize, Deserialize)]
pub struct OnboardingStatus {
    pub requires_setup: bool,
    pub user_count: i64,
    pub microsoft_auth_enabled: bool,
    pub oidc_enabled: bool,
    pub oidc_display_name: Option<String>,
    /// True when local credential auth (password + passkey) is disabled and
    /// the platform OIDC is the only sign-in path (hosted mode). The login
    /// UI hides the password/passkey forms and auto-initiates SSO.
    pub local_auth_disabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct AdminSetupRequest {
    pub name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminSetupResponse {
    pub success: bool,
    pub message: String,
    pub user: Option<UserResponse>,
}

// === MFA (Multi-Factor Authentication) Models ===

/// QR code matrix data for frontend rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrMatrix {
    /// Width/height of the QR code (always square)
    pub size: usize,
    /// Flattened boolean array (row-major order), true = dark module
    pub data: Vec<bool>,
}

/// Response for MFA setup request
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaSetupResponse {
    pub secret: String,
    pub qr_code: String,
    pub backup_codes: Vec<String>,
    /// QR code matrix data for animated rendering
    pub qr_matrix: Option<QrMatrix>,
}

/// Request for verifying MFA setup
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaVerifySetupRequest {
    pub token: String,
    pub secret: String,
}

/// Response for MFA setup verification
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaVerifySetupResponse {
    pub success: bool,
    pub backup_codes: Vec<String>,
}

/// Request for enabling MFA. The TOTP secret is intentionally NOT
/// in this struct (see `MfaEnableLoginRequest` for the threat model);
/// it lives in the server-side setup cache keyed by user uuid.
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaEnableRequest {
    pub token: String,
    pub backup_codes: Option<Vec<String>>,
}

/// Request for disabling MFA
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaDisableRequest {
    pub password: String,
}

/// Step-up credential for "sign out all other sessions". The caller
/// supplies whichever they have: a local password, or a TOTP / backup
/// code. Both optional so an OAuth-only account with no MFA (nothing to
/// step up with) can still call the endpoint on a full session.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RevokeOtherSessionsRequest {
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub mfa_code: Option<String>,
}

/// Request for regenerating backup codes
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaRegenerateBackupCodesRequest {
    pub password: String,
}

/// Response for regenerating backup codes
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaRegenerateBackupCodesResponse {
    pub backup_codes: Vec<String>,
}

/// Response for MFA status
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaStatusResponse {
    pub enabled: bool,
    pub has_backup_codes: bool,
}

/// Update struct for user MFA fields. Recovery codes live in
/// `user_recovery_codes` now — see
/// `repository::user_recovery_codes::replace_all` for the atomic
/// "rotate codes" operation that used to be a JSONB array swap on
/// this row.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = crate::schema::users)]
pub struct UserMfaUpdate {
    pub mfa_secret: Option<Option<Vec<u8>>>,
    pub mfa_secret_kek_id: Option<Option<i16>>,
    pub mfa_enabled: Option<bool>,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

// ===== PASSWORD RESET MODELS =====

/// Request to initiate password reset
#[derive(Debug, Serialize, Deserialize)]
pub struct PasswordResetRequest {
    pub email: String,
}

/// Response for password reset initiation
#[derive(Debug, Serialize, Deserialize)]
pub struct PasswordResetResponse {
    pub message: String,
}

/// Request to complete password reset with token
#[derive(Debug, Serialize, Deserialize)]
pub struct PasswordResetCompleteRequest {
    pub token: String,
    pub new_password: String,
}

// ===== INVITATION MODELS =====

/// Request to accept an invitation and set password
#[derive(Debug, Serialize, Deserialize)]
pub struct AcceptInvitationRequest {
    pub token: String,
    pub password: String,
}

/// Response for invitation acceptance
#[derive(Debug, Serialize, Deserialize)]
pub struct AcceptInvitationResponse {
    pub success: bool,
    pub message: String,
}

/// Request to validate an invitation token (check if it's valid before showing the form)
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidateInvitationRequest {
    pub token: String,
}

/// Response for invitation validation
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidateInvitationResponse {
    pub valid: bool,
    pub user_email: Option<String>,
    pub user_name: Option<String>,
    pub message: Option<String>,
    /// Who sent the invitation and into which workspace, from the metadata
    /// stamped at issue. Absent on older tokens and guest confirmations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invited_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
    /// Classification of the invitation's origin so the frontend can tailor
    /// copy ("confirm your ticket submission" vs generic onboarding).
    /// `"guest_ticket"` when the token was issued by a public ticket
    /// submission; `"invitation"` for an admin-sent invitation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Whether accepting sets a password. False for a guest confirmation
    /// where local credentials are disabled (hosted): the page confirms via
    /// `/invitation/confirm-guest` instead.
    #[serde(default)]
    pub password_required: bool,
}
