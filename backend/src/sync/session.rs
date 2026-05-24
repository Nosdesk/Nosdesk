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
    set_config(
        conn,
        "app.actor_ref",
        actor.reference.as_deref().unwrap_or(""),
    )?;
    set_config(conn, "app.correlation_id", &correlation_id)?;
    set_config(
        conn,
        "app.client_tx_id",
        actor.client_tx_id.as_deref().unwrap_or(""),
    )?;
    // `app.workspace_id` only gets overwritten when the actor
    // explicitly carries a workspace. If it doesn't (system jobs
    // built via `ActorContext::system(...)` that haven't been
    // pinned via `.with_workspace(...)`), we leave whatever the
    // surrounding context set. In production, the surrounding
    // context is the request's workspace middleware which sets
    // the GUC at outer-txn level; in tests, it's the bootstrap
    // workspace defaulted in `setup_test_connection`. Genuinely
    // cross-workspace background work uses
    // `with_actor_bypass_context` instead of relying on an unset
    // GUC, so the worst-case outcome here is the strict policy
    // returns zero rows when no ambient workspace exists (an
    // obvious empty-result bug, not a silent breach).
    if let Some(ws) = actor.workspace_id {
        set_config(conn, "app.workspace_id", &ws.to_string())?;
    }
    // Always reset the bypass flag on entry so a stale value can't
    // ride from a prior leaky txn (defensive; set_config with the
    // local flag is already txn-scoped). The bypass-context helper
    // sets this to "true" after calling set_actor.
    set_config(conn, "app.bypass_workspace_check", "")?;
    Ok(())
}

/// Run a closure inside a transaction with the actor GUCs primed, so
/// any `audit_log` triggers fired by the contained writes attribute
/// the change to `actor`. The GUCs are scoped to the transaction
/// (via `set_config(_, _, true)`); the transaction's commit or
/// rollback releases them.
///
/// Use this at the boundary where a handler calls into the repository
/// layer for an audited write:
///
/// ```ignore
/// sync::session::with_actor_context(&mut conn, &ctx.actor, |conn| {
///     repository::tickets::update(conn, ticket_id, patch)
/// })?;
/// ```
///
/// The actor argument is passed by reference so callers can keep
/// using their `RequestContext` afterwards. Returns the closure's
/// result transparently, including its error type, as long as that
/// error implements `From<diesel::result::Error>`.
pub fn with_actor_context<T, E>(
    conn: &mut DbConnection,
    actor: &ActorContext,
    f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
) -> Result<T, E>
where
    E: From<diesel::result::Error>,
{
    use diesel::Connection;

    conn.transaction(|conn| {
        set_actor(conn, actor)?;
        f(conn)
    })
}

/// Run a closure inside a transaction with the actor GUCs primed AND
/// `app.bypass_workspace_check = 'true'`, so RLS policies that read
/// the bypass flag let the query through regardless of workspace_id.
///
/// Reserved for legitimately cross-workspace operations: registry
/// sync, partition rotation, super-admin tools, the workspace
/// lifecycle handlers themselves. Every call site of this function
/// is greppable and audit-reviewable; ordinary handlers must use
/// `with_actor_context` instead.
///
/// The bypass flag is txn-scoped via `set_config(_, _, true)`, so it
/// can't leak between checkouts even if the surrounding code panics.
pub fn with_actor_bypass_context<T, E>(
    conn: &mut DbConnection,
    actor: &ActorContext,
    f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
) -> Result<T, E>
where
    E: From<diesel::result::Error>,
{
    use diesel::Connection;

    conn.transaction(|conn| {
        set_actor(conn, actor)?;
        set_config(conn, "app.bypass_workspace_check", "true")?;
        f(conn)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::actor::ActorContext;
    use crate::test_helpers::setup_test_connection;
    use diesel::sql_types::{Bool, Nullable, Text};

    #[derive(diesel::QueryableByName)]
    struct GucReadback {
        #[diesel(sql_type = Nullable<Text>)]
        current_setting: Option<String>,
    }

    fn read_guc(conn: &mut DbConnection, key: &str) -> Option<String> {
        // current_setting(key, true) returns NULL if the GUC isn't set,
        // rather than erroring — exactly what the audit triggers rely on.
        diesel::sql_query("SELECT current_setting($1, $2) AS current_setting")
            .bind::<Text, _>(key)
            .bind::<Bool, _>(true)
            .get_result::<GucReadback>(conn)
            .unwrap()
            .current_setting
            .filter(|s| !s.is_empty())
    }

    #[test]
    fn with_actor_context_threads_gucs_into_closure() {
        let mut conn = setup_test_connection();
        let user_uuid = uuid::Uuid::now_v7();
        let correlation_id = uuid::Uuid::now_v7();
        let actor = ActorContext::user(user_uuid, Some(correlation_id));

        with_actor_context(&mut conn, &actor, |conn| {
            assert_eq!(
                read_guc(conn, "app.actor_uuid"),
                Some(user_uuid.to_string())
            );
            assert_eq!(read_guc(conn, "app.actor_kind"), Some("user".to_string()));
            assert_eq!(
                read_guc(conn, "app.correlation_id"),
                Some(correlation_id.to_string())
            );
            Ok::<(), diesel::result::Error>(())
        })
        .expect("with_actor_context succeeded");
    }

    #[test]
    fn with_actor_context_system_actor_has_empty_uuid_guc() {
        let mut conn = setup_test_connection();
        let actor = ActorContext::system("scheduler.test");

        with_actor_context(&mut conn, &actor, |conn| {
            // Empty string -> treated as NULL by NULLIF in the trigger
            assert_eq!(read_guc(conn, "app.actor_uuid"), None);
            assert_eq!(read_guc(conn, "app.actor_kind"), Some("system".to_string()));
            assert_eq!(
                read_guc(conn, "app.actor_ref"),
                Some("scheduler.test".to_string())
            );
            Ok::<(), diesel::result::Error>(())
        })
        .expect("with_actor_context succeeded");
    }

    #[test]
    fn with_actor_context_propagates_closure_errors() {
        let mut conn = setup_test_connection();
        let actor = ActorContext::system("scheduler.test");

        let result: Result<(), diesel::result::Error> =
            with_actor_context(&mut conn, &actor, |_conn| {
                Err(diesel::result::Error::NotFound)
            });

        assert!(matches!(result, Err(diesel::result::Error::NotFound)));
    }
}
