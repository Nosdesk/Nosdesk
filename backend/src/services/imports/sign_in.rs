//! Done's sign-in line for a users import: who of the people it created
//! can't sign in yet, and how they will.
//!
//! An import never sends anything. A person it created has no password and
//! no sign-in identity, the same test the resend-invitation endpoint uses
//! for an account whose setup isn't finished. They count until they set a
//! password or sign in with a provider, and while they are still a member.
//!
//! How they sign in follows what the deployment offers:
//! - Self-hosted, where local credentials are allowed: an invitation sets
//!   their password. Done offers "Send invites", which goes to the people
//!   without an open invitation.
//! - Hosted: the import only creates requesters, who use the portal. With
//!   the workspace's requester sign-in provider enabled, they sign in with
//!   it once their accounts exist there. Without one, the portal emails
//!   them a sign-in link whenever they ask, so nobody is waiting and there
//!   is no line.

use std::collections::HashSet;

use serde::Serialize;
use uuid::Uuid;

use super::{RecordId, RecordIds};
use crate::db::DbConnection;
use crate::models::ImportJob;

/// The token type an invitation rides on.
pub const INVITATION: &str = "invitation";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInMethod {
    /// An invitation email sets their password.
    Invite,
    /// The workspace's sign-in provider, once their accounts exist there.
    Sso,
}

/// The line Done shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignInStatus {
    /// People the import created who can't sign in yet.
    pub pending: usize,
    /// Of those, the ones holding an invitation that is still open.
    pub invited: usize,
    pub method: SignInMethod,
    /// The provider's name, for `sso`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// For `invite`: whether email is set up, so invitations can be sent.
    pub email_ready: bool,
}

/// How the people sign in here: invitations where local credentials are
/// allowed, else the requester sign-in provider when one is enabled
/// (`Some(name)`), else nothing to wait for.
pub fn method(
    local_credentials: bool,
    requester_provider: Option<String>,
) -> Option<(SignInMethod, Option<String>)> {
    if local_credentials {
        Some((SignInMethod::Invite, None))
    } else {
        requester_provider.map(|name| (SignInMethod::Sso, Some(name)))
    }
}

/// The people a committed import created, by id.
pub fn created_people(job: &ImportJob) -> Vec<Uuid> {
    job.record_ids
        .clone()
        .and_then(|v| serde_json::from_value::<RecordIds>(v).ok())
        .map(|ids| {
            ids.created
                .into_iter()
                .filter_map(|id| match id {
                    RecordId::Uuid(u) => Some(u),
                    RecordId::Int(_) => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One person who can't sign in yet.
#[derive(Debug, Clone)]
pub struct Waiting {
    pub user: Uuid,
    pub email: String,
    /// Holds an invitation that is still open.
    pub invited: bool,
}

/// Of `people`, those still members of the pinned workspace with no
/// password and no sign-in identity, each with their primary email.
pub fn waiting(
    conn: &mut DbConnection,
    people: &[Uuid],
) -> Result<Vec<Waiting>, diesel::result::Error> {
    if people.is_empty() {
        return Ok(Vec::new());
    }
    let wanted: HashSet<Uuid> = people.iter().copied().collect();
    let members: Vec<(Uuid, String)> = crate::repository::workspaces::member_primary_emails(conn)?
        .into_iter()
        .filter(|(u, _)| wanted.contains(u))
        .collect();
    let uuids: Vec<Uuid> = members.iter().map(|(u, _)| *u).collect();
    let signed_in = crate::repository::user_auth_identities::users_with_sign_in(conn, &uuids)?;
    let invited =
        crate::repository::reset_tokens::users_with_live_tokens(conn, &uuids, INVITATION)?;
    // In the order the import created them.
    let mut out: Vec<Waiting> = members
        .into_iter()
        .filter(|(u, _)| !signed_in.contains(u))
        .map(|(user, email)| Waiting {
            invited: invited.contains(&user),
            user,
            email,
        })
        .collect();
    let order: std::collections::HashMap<Uuid, usize> =
        people.iter().enumerate().map(|(i, u)| (*u, i)).collect();
    out.sort_by_key(|w| order.get(&w.user).copied().unwrap_or(usize::MAX));
    Ok(out)
}

/// The requester sign-in provider's name, when the workspace has one
/// enabled.
fn requester_provider(conn: &mut DbConnection) -> Result<Option<String>, diesel::result::Error> {
    Ok(crate::repository::workspace_identity_providers::get(conn)?
        .filter(|p| p.enabled)
        .map(|p| p.display_name))
}

/// Whether the email service can send.
pub fn email_ready() -> bool {
    crate::utils::email::EmailService::from_env()
        .map(|s| s.is_configured())
        .unwrap_or(false)
}

/// Done's sign-in line for a committed users import, or `None` when there
/// is nobody to tell about.
pub fn status(
    conn: &mut DbConnection,
    job: &ImportJob,
) -> Result<Option<SignInStatus>, diesel::result::Error> {
    if job.job_type != "users" || job.status != "done" {
        return Ok(None);
    }
    let local = crate::middleware::workspace_context::local_credentials_permitted();
    let provider = if local {
        None
    } else {
        requester_provider(conn)?
    };
    let Some((method, provider)) = method(local, provider) else {
        return Ok(None);
    };
    let waiting = waiting(conn, &created_people(job))?;
    if waiting.is_empty() {
        return Ok(None);
    }
    Ok(Some(SignInStatus {
        pending: waiting.len(),
        invited: waiting.iter().filter(|w| w.invited).count(),
        method,
        provider,
        email_ready: method == SignInMethod::Invite && email_ready(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitations_where_local_credentials_are_allowed() {
        assert_eq!(method(true, None), Some((SignInMethod::Invite, None)));
        assert_eq!(
            method(true, Some("Contoso".into())),
            Some((SignInMethod::Invite, None))
        );
    }

    #[test]
    fn the_requester_provider_on_hosted_or_nothing() {
        assert_eq!(
            method(false, Some("Contoso".into())),
            Some((SignInMethod::Sso, Some("Contoso".into())))
        );
        assert_eq!(method(false, None), None);
    }
}
