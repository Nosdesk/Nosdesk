//! Whether a sign-in through the identity provider may open a session.
//!
//! Nosdesk asks for its own authenticator code on a password sign-in only; a
//! provider sign-in relies on the provider's checks. With `SSO_REQUIRE_MFA`
//! on, staff (anyone holding an agent, admin or owner seat, and platform
//! admins and audit reviewers) sign in through the provider only when its
//! id_token says it checked more than one factor: an `amr` value listed in
//! `SSO_MFA_METHODS` (default `mfa`, RFC 8176). Requesters are not affected.
//!
//! [`admit`] is the only way to make a [`SignInProof::Provider`], so no
//! provider sign-in opens a session without passing it.

use actix_web::HttpRequest;
use tracing::{error, warn};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{PlatformRole, User};
use crate::services::step_up::{ProviderClaims, SignInProof};

/// The operator's policy, read from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// `SSO_REQUIRE_MFA`: staff must sign in with multi-factor at the
    /// provider. Off by default.
    pub required: bool,
    /// `SSO_MFA_METHODS`: the `amr` values that count as multi-factor.
    pub methods: Vec<String>,
}

impl Policy {
    pub fn from_env() -> Self {
        Self::parse(
            std::env::var("SSO_REQUIRE_MFA").ok().as_deref(),
            std::env::var("SSO_MFA_METHODS").ok().as_deref(),
        )
    }

    fn parse(required: Option<&str>, methods: Option<&str>) -> Self {
        let required = matches!(
            required.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
            Some("1" | "true" | "yes" | "on")
        );
        let methods: Vec<String> = methods
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_owned)
            .collect();
        let methods = if methods.is_empty() {
            vec!["mfa".to_owned()]
        } else {
            methods
        };
        Self { required, methods }
    }

    /// Whether the provider's `amr` names a method that counts.
    fn reports_mfa(&self, amr: Option<&[String]>) -> bool {
        amr.unwrap_or_default().iter().any(|m| {
            self.methods
                .iter()
                .any(|counted| counted.eq_ignore_ascii_case(m))
        })
    }
}

/// A provider sign-in that [`admit`] let through. Its fields are private, so
/// nothing else can make one.
#[derive(Debug, Clone)]
pub struct AdmittedProvider<'a> {
    user_uuid: Uuid,
    id_token: Option<&'a str>,
    claims: ProviderClaims,
}

impl<'a> AdmittedProvider<'a> {
    /// The account it was admitted for.
    pub fn user_uuid(&self) -> Uuid {
        self.user_uuid
    }

    /// The provider id_token to keep on the session for sign-out.
    pub fn id_token(&self) -> Option<&'a str> {
        self.id_token
    }

    pub fn claims(&self) -> &ProviderClaims {
        &self.claims
    }

    #[cfg(test)]
    pub(crate) fn for_test(user_uuid: Uuid, id_token: Option<&'a str>) -> Self {
        Self {
            user_uuid,
            id_token,
            claims: id_token
                .map(ProviderClaims::from_id_token)
                .unwrap_or_default(),
        }
    }
}

/// Why [`admit`] refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The account is staff and the provider didn't report multi-factor.
    MfaNotReported,
    /// Whether the account is staff couldn't be read, so it was refused.
    Unknown,
}

/// Admit `user`'s sign-in through the provider under the operator's policy,
/// returning the proof its session is created with. A refusal is logged and
/// recorded as a `login_failed` security event.
pub fn admit<'a>(
    conn: &mut DbConnection,
    request: &HttpRequest,
    user: &User,
    id_token: Option<&'a str>,
    claims: ProviderClaims,
) -> Result<SignInProof<'a>, Refusal> {
    admit_under(&Policy::from_env(), conn, request, user, id_token, claims)
}

fn admit_under<'a>(
    policy: &Policy,
    conn: &mut DbConnection,
    request: &HttpRequest,
    user: &User,
    id_token: Option<&'a str>,
    claims: ProviderClaims,
) -> Result<SignInProof<'a>, Refusal> {
    let refusal = if !policy.required || policy.reports_mfa(claims.amr.as_deref()) {
        None
    } else {
        match is_staff(conn, user) {
            Ok(false) => None,
            Ok(true) => Some(Refusal::MfaNotReported),
            Err(e) => {
                error!(user_uuid = %user.uuid, error = %e, "SSO sign-in: reading the account's seats failed");
                Some(Refusal::Unknown)
            }
        }
    };
    let Some(refusal) = refusal else {
        return Ok(SignInProof::Provider(AdmittedProvider {
            user_uuid: user.uuid,
            id_token,
            claims,
        }));
    };
    warn!(user_uuid = %user.uuid, "SSO sign-in refused: staff must sign in with multi-factor at the provider");
    let _ = crate::utils::security_events::record_security_event(
        conn,
        crate::utils::security_events::SecurityEventInput {
            user_uuid: Some(user.uuid),
            event_type: "login_failed",
            severity: "warning",
            details: Some(serde_json::json!({
                "reason": match refusal {
                    Refusal::MfaNotReported => "provider_mfa_not_reported",
                    Refusal::Unknown => "provider_mfa_unknown_seats",
                },
                "amr": claims.amr,
            })),
            request: Some(request),
        },
    );
    Err(refusal)
}

/// Staff anywhere on this server. `staff_seat_holders` reads every workspace
/// from a tenant connection, so an unpinned connection can't hide a seat.
fn is_staff(conn: &mut DbConnection, user: &User) -> diesel::QueryResult<bool> {
    if PlatformRole::from_db(&user.platform_role) != PlatformRole::User {
        return Ok(true);
    }
    Ok(crate::repository::workspaces::staff_seat_holders(conn, &[user.uuid])?.contains(&user.uuid))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amr(values: &[&str]) -> Option<Vec<String>> {
        Some(values.iter().map(|v| (*v).to_owned()).collect())
    }

    #[test]
    fn the_policy_is_off_unless_turned_on_and_counts_mfa_by_default() {
        let off = Policy::parse(None, None);
        assert!(!off.required);
        assert_eq!(off.methods, vec!["mfa"]);
        assert!(Policy::parse(Some(" On "), None).required);
        assert!(!Policy::parse(Some("maybe"), None).required);
        assert_eq!(
            Policy::parse(Some("true"), Some(" otp, hwk ,,")).methods,
            vec!["otp", "hwk"]
        );
        assert_eq!(
            Policy::parse(Some("true"), Some(" , ")).methods,
            vec!["mfa"]
        );
    }

    #[test]
    fn staff_need_a_counted_method_and_requesters_do_not() {
        use crate::schema::security_events;
        use crate::test_helpers::{setup_test_connection, TestFixtures};
        use diesel::prelude::*;

        let mut conn = setup_test_connection();
        let agent = TestFixtures::create_user(&mut conn, "SSO Agent", "technician");
        let requester = TestFixtures::create_user(&mut conn, "SSO Requester", "user");
        let request = actix_web::test::TestRequest::default().to_http_request();
        let on = Policy::parse(Some("true"), None);
        let pwd = ProviderClaims {
            auth_time: None,
            amr: amr(&["pwd"]),
        };

        let refused = admit_under(&on, &mut conn, &request, &agent, None, pwd.clone());
        assert_eq!(refused.err(), Some(Refusal::MfaNotReported));
        let silent = ProviderClaims::default();
        assert!(admit_under(&on, &mut conn, &request, &agent, None, silent).is_err());
        let reasons: Vec<Option<serde_json::Value>> = security_events::table
            .filter(security_events::user_uuid.eq(agent.uuid))
            .filter(security_events::event_type.eq("login_failed"))
            .select(security_events::details)
            .load(&mut conn)
            .expect("events");
        assert_eq!(reasons.len(), 2);
        assert_eq!(
            reasons[0]
                .as_ref()
                .and_then(|d| d.get("reason"))
                .and_then(|r| r.as_str()),
            Some("provider_mfa_not_reported")
        );

        let mfa = ProviderClaims {
            auth_time: None,
            amr: amr(&["pwd", "mfa"]),
        };
        let proof = admit_under(&on, &mut conn, &request, &agent, Some("t"), mfa)
            .expect("multi-factor admits staff");
        assert_eq!(proof.id_token(), Some("t"));
        assert_eq!(
            proof.provider_amr(),
            Some(&["pwd".to_owned(), "mfa".to_owned()][..])
        );

        assert!(admit_under(&on, &mut conn, &request, &requester, None, pwd.clone()).is_ok());
        let off = Policy::parse(None, None);
        assert!(admit_under(&off, &mut conn, &request, &agent, None, pwd).is_ok());
    }

    #[test]
    fn only_a_counted_method_reports_mfa() {
        let policy = Policy::parse(Some("true"), None);
        assert!(policy.reports_mfa(amr(&["pwd", "mfa"]).as_deref()));
        assert!(policy.reports_mfa(amr(&["MFA"]).as_deref()));
        assert!(!policy.reports_mfa(amr(&["pwd"]).as_deref()));
        assert!(!policy.reports_mfa(amr(&[]).as_deref()));
        assert!(!policy.reports_mfa(None));

        let otp = Policy::parse(Some("true"), Some("otp,hwk"));
        assert!(otp.reports_mfa(amr(&["pwd", "otp"]).as_deref()));
        assert!(!otp.reports_mfa(amr(&["pwd", "mfa"]).as_deref()));
    }
}
