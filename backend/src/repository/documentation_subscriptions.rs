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
