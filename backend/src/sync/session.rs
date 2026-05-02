//! Session-local Postgres GUC threading.
//!
//! Sets `app.actor_uuid`, `app.actor_kind`, `app.actor_ref`,
//! `app.correlation_id`, and `app.client_tx_id` on the connection so
//! the audit_log trigger function (and `sync::emit::record`) can read
//! actor context without it being threaded through every repository
//! call.
//!
//! `set_config(key, value, true)` is the bind-parameter-friendly
//! equivalent of `SET LOCAL key = value`; the third argument scopes
//! the change to the current transaction. The wrapper must therefore
//! be called inside the same transaction as the writes it should
//! attribute. Typical use:
//!
//! ```ignore
//! conn.transaction(|conn| {
//!     sync::session::set_actor(conn, &actor)?;
//!     // ... writes that should be attributed to `actor`
//! })
//! ```
//!
//! When the GUC isn't set (background tasks that forgot to call
//! `set_actor`, tests that exercise repositories directly), every
//! consumer reads NULL via `current_setting('app.<key>', true)` and
//! the row still writes — so missing context never blocks a write.

use diesel::prelude::*;
use diesel::sql_types::{Bool, Nullable, Text};

use crate::db::DbConnection;
use crate::sync::actor::ActorContext;

#[derive(diesel::QueryableByName)]
struct DiscardSetConfig {
    #[diesel(sql_type = Nullable<Text>)]
    #[allow(dead_code)]
    set_config: Option<String>,
}

fn set_config(conn: &mut DbConnection, key: &str, value: &str) -> QueryResult<()> {
    diesel::sql_query("SELECT set_config($1, $2, $3) AS set_config")
        .bind::<Text, _>(key)
        .bind::<Text, _>(value)
        .bind::<Bool, _>(true)
        .get_result::<DiscardSetConfig>(conn)?;
    Ok(())
}

/// Set the session-local actor + correlation GUCs. Safe to call with
/// any actor kind; system actors set the actor_uuid GUC to empty,
/// which the trigger and emitter interpret as NULL via
/// `NULLIF(current_setting(...), '')`.
pub fn set_actor(conn: &mut DbConnection, actor: &ActorContext) -> QueryResult<()> {
    let actor_uuid = actor.uuid.map(|u| u.to_string()).unwrap_or_default();
    let correlation_id = actor
        .correlation_id
        .map(|u| u.to_string())
        .unwrap_or_default();

    set_config(conn, "app.actor_uuid", &actor_uuid)?;
    set_config(conn, "app.actor_kind", actor.kind.as_str())?;
    set_config(conn, "app.actor_ref", actor.reference.as_deref().unwrap_or(""))?;
    set_config(conn, "app.correlation_id", &correlation_id)?;
    set_config(conn, "app.client_tx_id", actor.client_tx_id.as_deref().unwrap_or(""))?;
    Ok(())
}
