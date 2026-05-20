//! User profile aggregation.
//!
//! Backs the `/api/users/{uuid}/profile` endpoint. The user
//! profile page in the frontend used to fan out to four separate
//! requests (user, devices, groups, emails) plus two ticket-list
//! fetches just to render badge counts; this module composes the
//! pieces into one bundle the page can ask for in a single call.
//!
//! Like `dashboard_stats`, the bundle uses sparse fieldsets:
//! callers pass `?include=...` and only the requested groups are
//! computed and serialised. The `user` field is always present
//! (it's the canonical resource the URL identifies), the rest are
//! `Option<...>` and skipped from JSON when not requested.

use std::collections::HashSet;

use diesel::dsl::count_star;
use diesel::prelude::*;
use diesel::QueryResult;
use serde::Serialize;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{Asset, Group, UserEmail, UserResponse};
use crate::repository::{
    assets as assets_repo, groups as groups_repo, user_emails as user_emails_repo, user_helpers,
    users as users_repo,
};
use crate::schema::tickets;

/// Discrete sub-resource groups the profile endpoint can return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileGroup {
    Devices,
    Groups,
    Emails,
    Counts,
}

impl ProfileGroup {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "devices" => Some(Self::Devices),
            "groups" => Some(Self::Groups),
            "emails" => Some(Self::Emails),
            "counts" => Some(Self::Counts),
            _ => None,
        }
    }

    pub fn all() -> HashSet<Self> {
        [Self::Devices, Self::Groups, Self::Emails, Self::Counts]
            .into_iter()
            .collect()
    }

    pub fn all_keys() -> &'static [&'static str] {
        &["devices", "groups", "emails", "counts"]
    }
}

/// Top-level response. `user` always present, other fields skipped
/// from JSON when not requested.
#[derive(Serialize)]
pub struct ProfileBundle {
    pub user: UserResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<Vec<Asset>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<Group>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emails: Option<Vec<UserEmail>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<ProfileCounts>,
}

/// Ticket counts scoped to this user, used to render badges
/// without loading full ticket lists.
#[derive(Serialize, Default)]
pub struct ProfileCounts {
    #[serde(rename = "assignedTickets")]
    pub assigned_tickets: i64,
    #[serde(rename = "requestedTickets")]
    pub requested_tickets: i64,
}

/// Build a bundle for `user_uuid` honouring the requested groups.
/// Returns `Ok(None)` if the user does not exist.
pub fn compute(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    groups: &HashSet<ProfileGroup>,
) -> QueryResult<Option<ProfileBundle>> {
    let user = match users_repo::get_user_by_uuid(user_uuid, conn) {
        Ok(u) => u,
        Err(diesel::result::Error::NotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    let user_response = user_helpers::get_user_with_primary_email(user, conn);

    let devices = if groups.contains(&ProfileGroup::Devices) {
        Some(assets_repo::get_devices_for_user(conn, user_uuid)?)
    } else {
        None
    };

    let groups_field = if groups.contains(&ProfileGroup::Groups) {
        Some(groups_repo::get_groups_for_user(conn, user_uuid)?)
    } else {
        None
    };

    let emails = if groups.contains(&ProfileGroup::Emails) {
        Some(user_emails_repo::get_user_emails_by_uuid(conn, user_uuid)?)
    } else {
        None
    };

    let counts = if groups.contains(&ProfileGroup::Counts) {
        Some(ticket_counts(conn, user_uuid)?)
    } else {
        None
    };

    Ok(Some(ProfileBundle {
        user: user_response,
        devices,
        groups: groups_field,
        emails,
        counts,
    }))
}

/// Two indexed counts. Backed by the partial indexes added in
/// `migrations/2026-04-27-000000_dashboard_stats_indexes`
/// (`idx_tickets_assignee_status_priority`,
/// `idx_tickets_requester_status_priority`).
fn ticket_counts(conn: &mut DbConnection, user_uuid: &Uuid) -> QueryResult<ProfileCounts> {
    let assigned: i64 = tickets::table
        .filter(tickets::assignee_uuid.eq(*user_uuid))
        .select(count_star())
        .first(conn)?;

    let requested: i64 = tickets::table
        .filter(tickets::requester_uuid.eq(*user_uuid))
        .select(count_star())
        .first(conn)?;

    Ok(ProfileCounts {
        assigned_tickets: assigned,
        requested_tickets: requested,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_keys() {
        assert_eq!(ProfileGroup::parse("devices"), Some(ProfileGroup::Devices));
        assert_eq!(ProfileGroup::parse("groups"), Some(ProfileGroup::Groups));
        assert_eq!(ProfileGroup::parse("emails"), Some(ProfileGroup::Emails));
        assert_eq!(ProfileGroup::parse("counts"), Some(ProfileGroup::Counts));
    }

    #[test]
    fn parse_unknown_key_rejected() {
        assert_eq!(ProfileGroup::parse("avatar"), None);
        assert_eq!(ProfileGroup::parse(""), None);
    }

    #[test]
    fn all_keys_round_trip_through_parse() {
        for key in ProfileGroup::all_keys() {
            assert!(
                ProfileGroup::parse(key).is_some(),
                "key {key} should round-trip through parse",
            );
        }
    }
}
