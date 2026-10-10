use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{DocumentationSubscription, NewDocumentationSubscription};
use crate::schema::documentation_subscriptions;

/// Get all user UUIDs subscribed to a given page
pub fn get_page_subscribers(conn: &mut DbConnection, page_id: i32) -> Vec<Uuid> {
    documentation_subscriptions::table
        .filter(documentation_subscriptions::page_id.eq(page_id))
        .select(documentation_subscriptions::user_uuid)
        .load::<Uuid>(conn)
        .unwrap_or_default()
}

/// Subscribers to `page_id` who may still read it. A subscription outlives a
/// change to the page's visibility, so who hears about an update is decided
/// when it is sent.
pub fn get_page_subscribers_who_can_read(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<Vec<Uuid>, diesel::result::Error> {
    use crate::models::{PlatformRole, WorkspaceRole};
    use crate::schema::users;

    let subscribers = get_page_subscribers(conn, page_id);
    if subscribers.is_empty() {
        return Ok(subscribers);
    }
    let platform_admins: std::collections::HashSet<Uuid> = users::table
        .filter(users::uuid.eq_any(&subscribers))
        .select((users::uuid, users::platform_role))
        .load::<(Uuid, String)>(conn)?
        .into_iter()
        .filter(|(_, role)| PlatformRole::from_db(role).is_platform_admin())
        .map(|(uuid, _)| uuid)
        .collect();
    let roles = crate::repository::user_helpers::workspace_roles_batch(&subscribers, conn);

    let mut readers = Vec::with_capacity(subscribers.len());
    for uuid in subscribers {
        let is_admin = platform_admins.contains(&uuid)
            || roles
                .get(&uuid)
                .is_some_and(|r| r.meets(WorkspaceRole::Admin));
        let audience = crate::repository::documentation::PageAudience::User {
            user_uuid: uuid,
            is_admin,
        };
        if audience.try_can_read(conn, page_id)? {
            readers.push(uuid);
        }
    }
    Ok(readers)
}

/// Check if a specific user is subscribed to a page
pub fn is_user_subscribed(conn: &mut DbConnection, user_uuid: Uuid, page_id: i32) -> bool {
    documentation_subscriptions::table
        .filter(documentation_subscriptions::user_uuid.eq(user_uuid))
        .filter(documentation_subscriptions::page_id.eq(page_id))
        .count()
        .get_result::<i64>(conn)
        .unwrap_or(0)
        > 0
}

// sync-pending-wire: needs sync aggregate wiring
/// Subscribe a user to a page
pub fn subscribe_user(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    page_id: i32,
) -> Result<DocumentationSubscription, diesel::result::Error> {
    let new_sub = NewDocumentationSubscription { user_uuid, page_id };
    diesel::insert_into(documentation_subscriptions::table)
        .values(&new_sub)
        .on_conflict((
            documentation_subscriptions::user_uuid,
            documentation_subscriptions::page_id,
        ))
        .do_nothing()
        .execute(conn)?;

    // Return the subscription (may have already existed)
    documentation_subscriptions::table
        .filter(documentation_subscriptions::user_uuid.eq(user_uuid))
        .filter(documentation_subscriptions::page_id.eq(page_id))
        .first(conn)
}

// sync-pending-wire: needs sync aggregate wiring
/// Unsubscribe a user from a page
pub fn unsubscribe_user(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    page_id: i32,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(
        documentation_subscriptions::table
            .filter(documentation_subscriptions::user_uuid.eq(user_uuid))
            .filter(documentation_subscriptions::page_id.eq(page_id)),
    )
    .execute(conn)
}
