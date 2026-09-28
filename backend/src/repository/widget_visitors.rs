//! Who a signed widget visitor is.
//!
//! The site's own id for the person (`sub`) is the identity: a
//! workspace-scoped identity with provider `widget`. The first time a `sub`
//! arrives:
//! - no account has the email: a requester is created (email left unverified,
//!   since a site's word isn't proof someone owns an inbox);
//! - an account has the email: it's joined only when the site says it
//!   verified the address (`email_verified`), and never if that account is
//!   already joined to a different `sub` here;
//! - staff are never signed in through a widget. A site's signing secret is
//!   weaker custody than an identity provider, so it doesn't reach them (unlike
//!   requester SSO, where the provider vouches for the address).

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;

/// The `provider_type` of a widget visitor's scoped identity.
pub const PROVIDER: &str = "widget";

/// What the site's token says about the visitor.
pub struct Visitor<'a> {
    pub sub: &'a str,
    pub email: &'a str,
    pub name: &'a str,
    pub email_verified: bool,
}

/// Why a visitor wasn't signed in.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Staff,
    /// The address belongs to an existing account and the site didn't say it
    /// verified it.
    Unverified,
    /// The address's account is joined to another `sub` here.
    BoundElsewhere,
}

impl Refusal {
    pub fn as_str(&self) -> &'static str {
        match self {
            Refusal::Staff => "staff account",
            Refusal::Unverified => "existing account, email not verified by the site",
            Refusal::BoundElsewhere => "email bound to another sub",
        }
    }
}

fn is_staff(conn: &mut DbConnection, workspace_id: i32, user: Uuid) -> QueryResult<bool> {
    use crate::schema::{users, workspace_members};
    let platform_admin: bool = users::table
        .find(user)
        .select(users::platform_role.eq("platform_admin"))
        .first(conn)
        .optional()?
        .unwrap_or(false);
    if platform_admin {
        return Ok(true);
    }
    diesel::select(diesel::dsl::exists(
        workspace_members::table
            .filter(workspace_members::workspace_id.eq(workspace_id))
            .filter(workspace_members::user_uuid.eq(user))
            .filter(workspace_members::role.eq_any(["owner", "admin", "agent"]))
            .filter(workspace_members::removed_at.is_null()),
    ))
    .get_result(conn)
}

// sync-audit-only: provisions the requester + seat + identity through their own audited repository writes
/// The visitor's account in `workspace_id` (created, with a member seat, when
/// needed), or why they can't be signed in. Run workspace-pinned.
pub fn resolve(
    conn: &mut DbConnection,
    workspace_id: i32,
    visitor: &Visitor<'_>,
) -> QueryResult<Result<Uuid, Refusal>> {
    use crate::repository::user_auth_identities as identities;
    use crate::schema::user_auth_identities as uai;

    if let Some(user) =
        identities::find_user_by_scoped_identity(workspace_id, PROVIDER, visitor.sub, conn)?
    {
        if is_staff(conn, workspace_id, user)? {
            return Ok(Err(Refusal::Staff));
        }
        return Ok(Ok(user));
    }

    let existing = match crate::repository::user_helpers::get_user_by_email(visitor.email, conn) {
        Ok(u) => Some(u),
        Err(diesel::result::Error::NotFound) => None,
        Err(e) => return Err(e),
    };
    let user = match existing {
        Some(user) => {
            if is_staff(conn, workspace_id, user.uuid)? {
                return Ok(Err(Refusal::Staff));
            }
            if !visitor.email_verified {
                return Ok(Err(Refusal::Unverified));
            }
            let bound_elsewhere: bool = diesel::select(diesel::dsl::exists(
                uai::table
                    .filter(uai::workspace_id.eq(workspace_id))
                    .filter(uai::provider_type.eq(PROVIDER))
                    .filter(uai::user_uuid.eq(user.uuid)),
            ))
            .get_result(conn)?;
            if bound_elsewhere {
                return Ok(Err(Refusal::BoundElsewhere));
            }
            user
        }
        None => crate::repository::user_helpers::find_or_provision_requester(
            visitor.email,
            visitor.name,
            conn,
            None,
        )?,
    };
    identities::create_identity(
        crate::models::NewUserAuthIdentity {
            user_uuid: user.uuid,
            provider_type: PROVIDER.to_string(),
            external_id: visitor.sub.to_string(),
            email: Some(visitor.email.to_string()),
            metadata: Some(json!({ "via": "widget" })),
            password_hash: None,
            workspace_id: Some(workspace_id),
        },
        conn,
    )?;
    // Already a member: a no-op (ON CONFLICT DO NOTHING).
    crate::repository::workspaces::add_membership(
        conn,
        workspace_id,
        user.uuid,
        "member",
        crate::repository::workspaces::SeatWriteAuthority::Product,
    )?;
    Ok(Ok(user.uuid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    fn v<'a>(sub: &'a str, email: &'a str, verified: bool) -> Visitor<'a> {
        Visitor {
            sub,
            email,
            name: "Visitor",
            email_verified: verified,
        }
    }

    #[test]
    fn the_sites_id_is_the_identity() {
        let mut conn = setup_test_connection();
        let first = resolve(&mut conn, 1, &v("acme-42", "sam@widget.test", false))
            .unwrap()
            .unwrap();
        let renamed = resolve(&mut conn, 1, &v("acme-42", "samuel@widget.test", false))
            .unwrap()
            .unwrap();
        assert_eq!(first, renamed, "same sub, same person");
        // Another sub claiming the first person's address can't reach them.
        assert_eq!(
            resolve(&mut conn, 1, &v("acme-99", "sam@widget.test", true)).unwrap(),
            Err(Refusal::BoundElsewhere)
        );
        assert!(crate::middleware::cookie_auth::is_workspace_member(
            &mut conn, 1, first
        ));
    }

    #[test]
    fn an_existing_account_is_joined_only_with_a_verified_email_and_never_staff() {
        let mut conn = setup_test_connection();
        let member = TestFixtures::create_user(&mut conn, "widget_member", "user");
        let email = "member@widget-existing.test".to_string();
        TestFixtures::create_user_email(&mut conn, member.uuid, &email, true);
        assert_eq!(
            resolve(&mut conn, 1, &v("site-1", &email, false)).unwrap(),
            Err(Refusal::Unverified)
        );
        assert_eq!(
            resolve(&mut conn, 1, &v("site-1", &email, true)).unwrap(),
            Ok(member.uuid)
        );

        let agent = TestFixtures::create_user(&mut conn, "widget_agent", "technician");
        let agent_email = "agent@widget-existing.test".to_string();
        TestFixtures::create_user_email(&mut conn, agent.uuid, &agent_email, true);
        assert_eq!(
            resolve(&mut conn, 1, &v("site-2", &agent_email, true)).unwrap(),
            Err(Refusal::Staff)
        );
    }
}
