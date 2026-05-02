//! Session-local Postgres GUC threading.
//!
//! Sets `app.actor_uuid` and `app.correlation_id` on the connection so
//! the audit_log trigger function (and any future plpgsql that wants
//! actor context) can see who's writing without a parameter passed
//! down through every Diesel call.
//!
//! `SET LOCAL` scopes the GUC to the current transaction, so this
//! helper must be called inside the same transaction as the writes
//! it should attribute. Call sites typically do:
//!
//! ```ignore
//! conn.transaction(|conn| {
//!     sync::session::set_actor(conn, &actor)?;
//!     // ... writes that should be attributed to `actor`
//! })
//! ```

use diesel::prelude::*;

use crate::db::DbConnection;
use crate::sync::actor::ActorContext;

/// Set the session-local actor + correlation GUCs. Safe to call with
/// any actor kind; system actors set the actor_uuid GUC to empty,
/// which the trigger interprets as NULL.
pub fn set_actor(conn: &mut DbConnection, actor: &ActorContext) -> QueryResult<()> {
    let actor_uuid = actor.uuid.map(|u| u.to_string()).unwrap_or_default();
    let correlation_id = actor
        .correlation_id
        .map(|u| u.to_string())
        .unwrap_or_default();

    // SET LOCAL doesn't accept bound parameters and Postgres's
    // prepared-statement protocol doesn't accept multiple commands in
    // one call, so we issue two separate executes. UUIDs are
    // well-bounded (hex + dashes only) so SQL injection isn't a
    // vector — but the values still go through Uuid::to_string above
    // rather than accepting raw strings from a caller.
    diesel::sql_query(format!("SET LOCAL app.actor_uuid = '{actor_uuid}'")).execute(conn)?;
    diesel::sql_query(format!(
        "SET LOCAL app.correlation_id = '{correlation_id}'"
    ))
    .execute(conn)?;
    Ok(())
}
