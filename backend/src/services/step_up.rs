//! Step-up confirmation. Actions with a high blast radius (signing out every
//! other session, deleting users) go ahead only when the session asking proved
//! who is using it within the last [`WINDOW_MINUTES`].
//!
//! The proof is `active_sessions.confirmed_at`, per session, so confirming in
//! one browser never covers another. What sets it:
//! - a sign-in that checked a credential (password, LDAP, passkey, code): the
//!   moment it did;
//! - a provider sign-in: the id_token's `auth_time` ([`ProviderClaims`]), so
//!   a remembered sign-in that asked for nothing proves nothing;
//! - a confirmation in the step-up dialog: password, authenticator code,
//!   passkey, or a fresh provider sign-in (`handlers::step_up`).
//!
//! Nothing else passes. An account with no way to confirm signs in again.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{StepUpCredential, User};
use crate::repository;
use crate::services::sso_policy::AdmittedProvider;

/// How long a confirmation covers: long enough to finish a task (an admin
/// removing several people confirms once), short enough that a stolen
/// session rarely lands inside one.
pub const WINDOW_MINUTES: i64 = 10;

/// How far a provider's clock may run ahead of ours when its `auth_time` is
/// compared with the moment a confirmation started.
pub const CLOCK_SKEW_SECS: i64 = 30;

/// The `max_age` a confirmation sends a provider. Asking for one obliges the
/// provider to return `auth_time`; `prompt=login` is what makes it ask for a
/// credential, and [`CLOCK_SKEW_SECS`] against the flow's start is what
/// refuses an older one. Not zero: Entra caps the id_token's `exp` at
/// `auth_time + max_age`, so `max_age=0` returns a token that has already
/// expired.
pub const PROVIDER_MAX_AGE_SECS: u64 = (WINDOW_MINUTES * 60) as u64;

/// What the sign-in that creates a session proved.
#[derive(Debug, Clone)]
pub enum SignInProof<'a> {
    /// A password, LDAP password, passkey or authenticator code, just now.
    Credential,
    /// A provider sign-in, admitted by the SSO policy
    /// ([`crate::services::sso_policy::admit`], the only way to make one).
    Provider(AdmittedProvider<'a>),
    /// Nothing checked here (a legacy refresh, a portal sign-in).
    Nothing,
}

impl<'a> SignInProof<'a> {
    /// The session's `confirmed_at` at creation: a provider sign-in counts
    /// from its `auth_time`, absent when the provider doesn't say.
    pub fn confirmed_at(&self, now: DateTime<Utc>) -> Option<NaiveDateTime> {
        match self {
            Self::Credential => Some(now.naive_utc()),
            // A provider clock ahead of ours must not stretch the window.
            Self::Provider(p) => p.claims().auth_time.map(|t| t.min(now).naive_utc()),
            Self::Nothing => None,
        }
    }

    /// The provider id_token to keep on the session.
    pub fn id_token(&self) -> Option<&'a str> {
        match self {
            Self::Provider(p) => p.id_token(),
            _ => None,
        }
    }

    /// How the provider checked who signed in (`amr`), to keep on the
    /// session. `None` for any other sign-in.
    pub fn provider_amr(&self) -> Option<&[String]> {
        match self {
            Self::Provider(p) => Some(p.claims().amr.as_deref().unwrap_or_default()),
            _ => None,
        }
    }
}

/// What a provider's id_token says about the sign-in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderClaims {
    /// When the provider last checked a credential.
    pub auth_time: Option<DateTime<Utc>>,
    /// How it checked (`amr`, RFC 8176), in the provider's own words.
    /// `None` when the token doesn't say.
    pub amr: Option<Vec<String>>,
}

impl ProviderClaims {
    /// Read from an id_token. No signature check: callers pass a token they
    /// just verified, or the copy kept on their own session row.
    pub fn from_id_token(id_token: &str) -> Self {
        use base64::Engine as _;
        #[derive(serde::Deserialize)]
        struct Claims {
            auth_time: Option<i64>,
            amr: Option<Vec<String>>,
        }
        let claims = id_token
            .split('.')
            .nth(1)
            .and_then(|p| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(p.trim_end_matches('='))
                    .ok()
            })
            .and_then(|b| serde_json::from_slice::<Claims>(&b).ok());
        let Some(claims) = claims else {
            return Self::default();
        };
        Self {
            auth_time: claims
                .auth_time
                .and_then(|t| DateTime::from_timestamp(t, 0)),
            amr: claims.amr,
        }
    }
}

/// Until when a session confirmed at `confirmed_at` stays confirmed, or
/// `None` once that has passed.
pub fn confirmed_until(
    confirmed_at: Option<NaiveDateTime>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let until = confirmed_at?.and_utc() + chrono::Duration::minutes(WINDOW_MINUTES);
    (until > now).then_some(until)
}

/// Until when session `sid` of `user_uuid` stays confirmed.
pub fn session_confirmed_until(
    conn: &mut DbConnection,
    sid: &Uuid,
    user_uuid: &Uuid,
) -> Option<DateTime<Utc>> {
    match repository::active_sessions::confirmed_at(conn, sid, user_uuid) {
        Ok(at) => confirmed_until(at, Utc::now()),
        Err(e) => {
            tracing::error!(error = %e, "step-up: reading the session's confirmation failed");
            None
        }
    }
}

/// Record a confirmation at `at` on session `sid`, keeping `id_token` as
/// its provider id_token when given. Returns until when it is confirmed.
pub fn record(
    conn: &mut DbConnection,
    sid: &Uuid,
    user_uuid: &Uuid,
    at: DateTime<Utc>,
    id_token: Option<&str>,
) -> Option<DateTime<Utc>> {
    let at = at.min(Utc::now()).naive_utc();
    if let Err(e) =
        repository::active_sessions::record_confirmation(conn, sid, user_uuid, at, id_token)
    {
        tracing::error!(error = %e, "step-up: recording the confirmation failed");
    }
    session_confirmed_until(conn, sid, user_uuid)
}

/// Credential attempts allowed per person in [`ATTEMPT_WINDOW_SECS`], counted
/// across the dialog and the actions that take a credential with the request.
/// Every attempt counts, right or wrong.
pub const ATTEMPTS_PER_WINDOW: u32 = 10;
pub const ATTEMPT_WINDOW_SECS: u64 = 300;

/// Counts an attempt by `user_uuid` and says whether it is over
/// [`ATTEMPTS_PER_WINDOW`]. When the limiter can't be read the attempt goes
/// ahead (logged): the credential is still checked, and a Redis outage
/// shouldn't stop every admin confirming.
pub async fn attempts_exhausted(user_uuid: &Uuid) -> bool {
    use crate::utils::rate_limit::{get_redis_url, RateLimiter};
    let key = format!("step_up_attempts:{user_uuid}");
    match RateLimiter::check_rate_limit(
        &get_redis_url(),
        &key,
        ATTEMPTS_PER_WINDOW,
        ATTEMPT_WINDOW_SECS,
    )
    .await
    {
        Ok(allowed) => !allowed,
        Err(e) => {
            tracing::warn!(error = ?e, "step-up: rate limit check failed");
            false
        }
    }
}

/// The 429 for [`attempts_exhausted`].
pub fn too_many_attempts() -> actix_web::HttpResponse {
    crate::errors::too_many_requests(
        "Too many attempts. Try again in a few minutes.",
        ATTEMPT_WINDOW_SECS,
    )
}

/// [`record`] a confirmation by `method` now, and log it as a security event.
/// Returns until when the session is confirmed.
pub fn record_confirmed(
    conn: &mut DbConnection,
    req: &actix_web::HttpRequest,
    user_uuid: &Uuid,
    sid: &Uuid,
    method: Method,
) -> Option<DateTime<Utc>> {
    let until = record(conn, sid, user_uuid, Utc::now(), None)?;
    let _ = crate::utils::security_events::record_security_event(
        conn,
        crate::utils::security_events::SecurityEventInput {
            user_uuid: Some(*user_uuid),
            event_type: "step_up_confirmed",
            severity: "info",
            details: Some(
                serde_json::json!({ "method": method.as_str(), "session_id": sid.to_string() }),
            ),
            request: Some(req),
        },
    );
    Some(until)
}

/// A way to confirm, in the order the dialog prefers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// The local password, or the directory password of an LDAP account.
    Password,
    Passkey,
    /// A fresh sign-in at the account's identity provider, in a popup.
    Provider,
    /// An authenticator or backup code.
    Code,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Passkey => "passkey",
            Self::Provider => "provider",
            Self::Code => "code",
        }
    }
}

/// The identity provider an account can confirm with.
#[derive(Debug, Clone, Serialize)]
pub struct StepUpProvider {
    /// `oidc` or `microsoft`, as in `/oauth/authorize`.
    pub provider_type: String,
    /// What the dialog calls it ("Continue with {name}").
    pub name: String,
}

/// How `user` can confirm, best first, and their provider when they have one.
pub fn methods(conn: &mut DbConnection, user: &User) -> (Vec<Method>, Option<StepUpProvider>) {
    let local = crate::middleware::workspace_context::local_credentials_permitted();
    let mut methods = Vec::new();
    if local
        && (repository::user_auth_identities::get_local_password_hash(&user.uuid, conn).is_ok()
            || ldap_identity(conn, &user.uuid).is_some())
    {
        methods.push(Method::Password);
    }
    if local && repository::passkey_credentials::count_for_user(conn, &user.uuid).unwrap_or(0) > 0 {
        methods.push(Method::Passkey);
    }
    let provider = crate::handlers::auth_providers::step_up_provider(conn, &user.uuid);
    if provider.is_some() {
        methods.push(Method::Provider);
    }
    if user.mfa_enabled {
        methods.push(Method::Code);
    }
    (methods, provider)
}

/// The account's LDAP identity: `(workspace_id, external_id)`.
fn ldap_identity(conn: &mut DbConnection, user_uuid: &Uuid) -> Option<(i32, String)> {
    repository::user_auth_identities::get_user_identities(user_uuid, conn)
        .ok()?
        .into_iter()
        .find(|i| i.provider_type == "ldap")
        .and_then(|i| Some((i.workspace_id?, i.external_id)))
}

/// Check a credential typed into the step-up dialog. Returns the method it
/// confirmed with, or `None` when it is empty or wrong.
pub async fn check_credential(
    conn: &mut DbConnection,
    user: &User,
    credential: &StepUpCredential,
) -> Option<Method> {
    if let Some(password) = credential.password.as_deref().filter(|p| !p.is_empty()) {
        if !crate::middleware::workspace_context::local_credentials_permitted() {
            return None;
        }
        if let Ok(hash) =
            repository::user_auth_identities::get_local_password_hash(&user.uuid, conn)
        {
            if bcrypt::verify(password, &hash).unwrap_or(false) {
                return Some(Method::Password);
            }
        }
        return ldap_password_matches(conn, &user.uuid, password)
            .await
            .then_some(Method::Password);
    }
    let code = credential
        .mfa_code
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())?;
    let valid = user.mfa_enabled
        && crate::utils::mfa::verify_mfa_token(&user.uuid, code, conn)
            .await
            .map(|r| r.is_valid)
            .unwrap_or(false);
    valid.then_some(Method::Code)
}

/// Whether `password` binds as the account's own directory entry.
async fn ldap_password_matches(conn: &mut DbConnection, user_uuid: &Uuid, password: &str) -> bool {
    let Some((workspace_id, external_id)) = ldap_identity(conn, user_uuid) else {
        return false;
    };
    let settings = match repository::workspace_ldap_settings::get_for_workspace(conn, workspace_id)
    {
        Ok(Some(s)) if s.enabled => s,
        Ok(_) => return false,
        Err(e) => {
            tracing::error!(error = %e, "step-up: loading LDAP settings failed");
            return false;
        }
    };
    let bind_password = repository::workspace_ldap_settings::decrypt_bind_password(&settings)
        .ok()
        .flatten()
        .unwrap_or_default();
    match crate::services::ldap::auth::authenticate_external_id(
        &settings,
        &bind_password,
        &external_id,
        password,
    )
    .await
    {
        Ok(_) => true,
        Err(crate::services::ldap::auth::LdapAuthError::InvalidCredentials)
        | Err(crate::services::ldap::auth::LdapAuthError::EmptyPassword) => false,
        Err(e) => {
            tracing::warn!(error = %e, workspace_id, "step-up: LDAP password check failed");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_with(payload: serde_json::Value) -> String {
        use base64::Engine as _;
        let enc = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.sig",
            enc(br#"{"alg":"RS256"}"#),
            enc(payload.to_string().as_bytes())
        )
    }

    fn provider(id_token: &str) -> SignInProof<'_> {
        SignInProof::Provider(AdmittedProvider::for_test(Uuid::nil(), Some(id_token)))
    }

    #[test]
    fn a_confirmation_lasts_the_window_and_no_longer() {
        let now = Utc::now();
        let at = |mins: i64| Some((now - chrono::Duration::minutes(mins)).naive_utc());
        assert!(confirmed_until(at(0), now).is_some());
        assert!(confirmed_until(at(WINDOW_MINUTES - 1), now).is_some());
        assert!(confirmed_until(at(WINDOW_MINUTES), now).is_none());
        assert!(confirmed_until(None, now).is_none());
    }

    #[test]
    fn a_provider_sign_in_counts_from_when_it_checked_a_credential() {
        let now = Utc::now();
        let earlier = now.timestamp() - 7 * 3600;
        let remembered = token_with(serde_json::json!({ "sub": "u", "auth_time": earlier }));
        let proof = provider(&remembered);
        assert_eq!(
            proof.confirmed_at(now).map(|t| t.and_utc().timestamp()),
            Some(earlier),
        );
        assert!(confirmed_until(proof.confirmed_at(now), now).is_none());
        assert_eq!(proof.id_token(), Some(remembered.as_str()));
    }

    #[test]
    fn a_provider_sign_in_without_auth_time_proves_nothing() {
        let token = token_with(serde_json::json!({ "sub": "u" }));
        assert_eq!(provider(&token).confirmed_at(Utc::now()), None);
    }

    #[test]
    fn a_provider_clock_ahead_of_ours_does_not_stretch_the_window() {
        let now = Utc::now();
        let ahead = token_with(serde_json::json!({ "auth_time": now.timestamp() + 3600 }));
        let at = provider(&ahead).confirmed_at(now).unwrap();
        assert!(at <= now.naive_utc());
    }

    #[test]
    fn a_provider_sign_in_keeps_the_methods_the_provider_named() {
        let token = token_with(serde_json::json!({ "sub": "u", "amr": ["pwd", "mfa"] }));
        assert_eq!(
            provider(&token).provider_amr(),
            Some(&["pwd".to_owned(), "mfa".to_owned()][..])
        );
        let silent = token_with(serde_json::json!({ "sub": "u" }));
        assert_eq!(ProviderClaims::from_id_token(&silent).amr, None);
        assert_eq!(provider(&silent).provider_amr(), Some(&[][..]));
        assert_eq!(SignInProof::Credential.provider_amr(), None);
        assert_eq!(
            ProviderClaims::from_id_token("not-a-jwt"),
            ProviderClaims::default()
        );
    }

    #[test]
    fn credential_sign_ins_count_and_others_do_not() {
        let now = Utc::now();
        assert_eq!(
            SignInProof::Credential.confirmed_at(now),
            Some(now.naive_utc())
        );
        assert_eq!(SignInProof::Nothing.confirmed_at(now), None);
    }
}
