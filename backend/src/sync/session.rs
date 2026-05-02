//! Session-local Postgres GUC threading.
//!
//! Sets `app.actor_uuid`, `app.actor_kind`, `app.actor_ref`,
//! `app.correlation_id`, and `app.client_tx_id` on the connection so
//! the audit_log trigger function (and `sync::emit::record`) can read
//! actor context without it being threaded through every repository
//! call.
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
//!
//! For the common single-write case where the entire handler is one
//! "transaction" of work, callers can also call `set_actor` once per
//! pooled connection checkout (outside any transaction wrapper) — but
//! that mixes the GUC scope across requests if the connection is
//! returned to the pool and handed to a different request, which is
//! why the trigger and emitter never assume the GUC is set and fall
//! back to NULL for missing values.

use diesel::prelude::*;

use crate::db::DbConnection;
use crate::sync::actor::ActorContext;

/// Set the session-local actor + correlation GUCs. Safe to call with
/// any actor kind; system actors set the actor_uuid GUC to empty,
/// which the trigger and emitter interpret as NULL.
pub fn set_actor(conn: &mut DbConnection, actor: &ActorContext) -> QueryResult<()> {
    // SET LOCAL doesn't accept bound parameters and Postgres's
    // prepared-statement protocol doesn't accept multiple commands in
    // one call, so we issue separate executes. The values that come
    // from typed sources (Uuid, ActorKind enum) are safe to
    // interpolate; the free-form `reference` is sanitised to keep a
    // mistyped plugin slug from terminating the literal.
    let actor_uuid = actor.uuid.map(|u| u.to_string()).unwrap_or_default();
    let correlation_id = actor
        .correlation_id
        .map(|u| u.to_string())
        .unwrap_or_default();
    let client_tx_id = actor.client_tx_id.as_deref().unwrap_or("");
    let actor_ref = actor.reference.as_deref().unwrap_or("");

    diesel::sql_query(format!("SET LOCAL app.actor_uuid = '{actor_uuid}'")).execute(conn)?;
    diesel::sql_query(format!("SET LOCAL app.actor_kind = '{}'", actor.kind.as_str()))
        .execute(conn)?;
    diesel::sql_query(format!(
        "SET LOCAL app.actor_ref = '{}'",
        sanitise_guc(actor_ref)
    ))
    .execute(conn)?;
    diesel::sql_query(format!(
        "SET LOCAL app.correlation_id = '{correlation_id}'"
    ))
    .execute(conn)?;
    diesel::sql_query(format!(
        "SET LOCAL app.client_tx_id = '{}'",
        sanitise_guc(client_tx_id)
    ))
    .execute(conn)?;
    Ok(())
}

/// Strip single-quote characters and backslashes from values that go
/// into `SET LOCAL` literals. The sanctioned values are slugs, UUIDs,
/// and short identifiers; nothing that would legitimately contain
/// these characters reaches this path.
fn sanitise_guc(s: &str) -> String {
    s.chars().filter(|c| *c != '\'' && *c != '\\').collect()
}
