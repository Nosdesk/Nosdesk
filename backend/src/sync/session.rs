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
    // Re-establish the baseline role at the start of every
    // actor-context txn. In production this is a no-op
    // (`nosdesk_app` is already the connection's auth role); in
    // tests it counteracts a prior `with_actor_bypass_context`
    // call that elevated to `nosdesk_admin` inside a savepoint
    // (savepoint commit promotes the SET LOCAL into the outer
    // txn's scope, so without this reset the role would persist
    // until the test's begin_test_transaction unwinds). This is
    // the substitute for the old `app.bypass_workspace_check`
    // reset, which Phase 3h.4 removed when bypass moved from a
    // GUC flag to a separate BYPASSRLS role.
    diesel::sql_query("SET LOCAL ROLE nosdesk_app").execute(conn)?;
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
        let value = f(conn)?;
        // Backstop against silent write loss. If `f` returned Ok but left the
        // transaction in an aborted state (a query error that was swallowed
        // rather than propagated — e.g. a stale-schema read behind an
        // `unwrap_or_default`), the COMMIT that `transaction` is about to issue
        // gets downgraded to a ROLLBACK by Postgres while still reporting
        // success: the writes vanish behind a 2xx. Probe with a trivial
        // statement so an aborted transaction surfaces as a real error here and
        // rolls back loudly instead of committing nothing.
        diesel::sql_query("SELECT 1").execute(conn)?;
        Ok(value)
    })
}

/// [`with_actor_context`] for closures whose error type is `String`.
///
/// Several service and repository layers (OAuth provisioning, webhook
/// delivery) surface `Result<T, String>` rather than a diesel error,
/// so they can't satisfy `with_actor_context`'s `E: From<Error>`
/// bound directly. This wrapper runs the closure inside the actor
/// transaction, rolling back on the closure's `Err` and returning the
/// original `String` verbatim. A failure to set the actor GUCs
/// themselves (rare) is stringified.
pub fn with_actor_context_str<T>(
    conn: &mut DbConnection,
    actor: &ActorContext,
    f: impl FnOnce(&mut DbConnection) -> Result<T, String>,
) -> Result<T, String> {
    let mut captured: Option<Result<T, String>> = None;
    let outcome = with_actor_context(conn, actor, |c| match f(c) {
        Ok(v) => {
            captured = Some(Ok(v));
            Ok(())
        }
        Err(e) => {
            captured = Some(Err(e));
            // Abort the transaction so partial writes roll back; the
            // real error is preserved in `captured`.
            Err(diesel::result::Error::RollbackTransaction)
        }
    });
    match outcome {
        // Ok, or the intentional rollback we triggered above: the
        // closure ran, so `captured` holds the real result.
        Ok(()) | Err(diesel::result::Error::RollbackTransaction) => {
            captured.unwrap_or_else(|| Err("actor-context transaction did not run".to_string()))
        }
        // set_actor (or the transaction machinery) failed before the
        // closure produced a result.
        Err(other) => Err(format!("actor context setup failed: {other}")),
    }
}

/// Set up the connection for an async-friendly elevated session:
/// session-scoped `SET ROLE nosdesk_admin` plus session-scoped
/// actor + workspace GUCs.
///
/// Companion to [`reset_session_role`]. Use this pair when the
/// caller is async code that mixes DB ops with `.await` points
/// (channels poll loop, msgraph perform_sync) and therefore can
/// neither fit inside [`background_run`]'s sync-closure boundary
/// nor inside [`with_actor_bypass_context`]'s txn boundary.
///
/// Trade-off vs `background_run`: these settings persist for the
/// connection's lifetime, not just one txn. The caller MUST invoke
/// `reset_session_role` before releasing the connection back to
/// the pool; otherwise the elevation leaks across requests. If
/// the async block panics, the leaked state is the cost of doing
/// business — pair this helper with an actix `spawn` that catches
/// panics, or a guard struct, to recover.
///
/// Reserved for the small set of call sites that genuinely need
/// the session shape. Handlers should use `TenantConn` /
/// `PlatformConn`; sync-closure spawn tasks should use
/// `background_run`.
pub fn elevate_session_role(conn: &mut DbConnection, actor: &ActorContext) -> QueryResult<()> {
    // SET ROLE (without LOCAL) persists for the connection's
    // session. The companion reset_session_role calls RESET ROLE.
    diesel::sql_query("SET ROLE nosdesk_admin").execute(conn)?;
    set_actor_session_scoped(conn, actor)
}

/// Inverse of [`elevate_session_role`]: returns the role to the
/// connection's login role (typically `nosdesk_app`) and clears
/// every actor / workspace GUC the elevation set. Best-effort —
/// each failure is logged so the caller can still release the
/// connection without bubbling up an error.
pub fn reset_session_role(conn: &mut DbConnection) {
    if let Err(e) = diesel::sql_query("RESET ROLE").execute(conn) {
        tracing::warn!(error = %e, "RESET ROLE failed in reset_session_role");
    }
    const KEYS: &[&str] = &[
        "app.actor_uuid",
        "app.actor_kind",
        "app.actor_ref",
        "app.correlation_id",
        "app.client_tx_id",
        "app.workspace_id",
    ];
    for key in KEYS {
        if let Err(e) = diesel::sql_query("SELECT set_config($1, '', false) AS set_config")
            .bind::<Text, _>(*key)
            .get_result::<DiscardSetConfig>(conn)
        {
            tracing::warn!(key = %key, error = %e, "clearing GUC failed in reset_session_role");
        }
    }
}

fn set_actor_session_scoped(conn: &mut DbConnection, actor: &ActorContext) -> QueryResult<()> {
    let actor_uuid = actor.uuid.map(|u| u.to_string()).unwrap_or_default();
    let correlation_id = actor
        .correlation_id
        .map(|u| u.to_string())
        .unwrap_or_default();

    set_config_session(conn, "app.actor_uuid", &actor_uuid)?;
    set_config_session(conn, "app.actor_kind", actor.kind.as_str())?;
    set_config_session(
        conn,
        "app.actor_ref",
        actor.reference.as_deref().unwrap_or(""),
    )?;
    set_config_session(conn, "app.correlation_id", &correlation_id)?;
    set_config_session(
        conn,
        "app.client_tx_id",
        actor.client_tx_id.as_deref().unwrap_or(""),
    )?;
    if let Some(ws) = actor.workspace_id {
        set_config_session(conn, "app.workspace_id", &ws.to_string())?;
    }
    Ok(())
}

fn set_config_session(conn: &mut DbConnection, key: &str, value: &str) -> QueryResult<()> {
    diesel::sql_query("SELECT set_config($1, $2, false) AS set_config")
        .bind::<Text, _>(key)
        .bind::<Text, _>(value)
        .get_result::<DiscardSetConfig>(conn)?;
    Ok(())
}

/// Run a closure inside a transaction with the actor GUCs primed AND
/// elevated to the `nosdesk_admin` BYPASSRLS role, so every RLS
/// policy is skipped for the txn.
///
/// Reserved for legitimately cross-workspace operations: registry
/// sync, partition rotation, super-admin tools, the workspace
/// lifecycle handlers themselves. Every call site of this function
/// is greppable and audit-reviewable; ordinary handlers must use
/// `with_actor_context` instead.
///
/// Bypass is enforced by Postgres role membership, not by a GUC
/// flag the application code could trip on accidentally (Phase
/// 3h.4 moved off the GUC scheme because placeholder `app.*` GUCs
/// are write-by-any-role and the bypass was therefore convention-
/// enforced, not DB-enforced). `SET LOCAL ROLE` is txn-scoped, so
/// the elevation evaporates at commit / rollback. Set-actor's
/// baseline reset (`SET LOCAL ROLE nosdesk_app`) at the start of
/// every actor-context txn defends against role-leak across
/// sequential savepoints in tests.
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
        // set_actor runs first; it includes the baseline-role
        // reset (`SET LOCAL ROLE nosdesk_app`). Then we elevate.
        // Order matters: if we SET ROLE before set_actor, the
        // baseline reset inside set_actor would undo the
        // elevation.
        set_actor(conn, actor)?;
        diesel::sql_query("SET LOCAL ROLE nosdesk_admin").execute(conn)?;
        f(conn)
    })
}

/// Convenience wrapper for background-task DB work: acquire a
/// pooled connection, elevate to `nosdesk_admin` (BYPASSRLS) for
/// the closure's duration, run the closure inside one
/// transaction, return the result.
///
/// Used by schedulers, spawned workers, webhook / notification
/// dispatchers, and similar code that runs outside any HTTP
/// request context. The `reference` string is captured on every
/// emitted sync_actions row and (via the actor GUC) on every
/// audit_log row, giving operators a grep target to distinguish
/// background traffic from request traffic.
///
/// # Reference prefix convention
///
/// Pick the prefix that matches the call site, not the table
/// being written. The prefix tells an on-call investigator
/// *where* a row came from when they're staring at
/// `SELECT actor_ref FROM audit_log WHERE ...`:
///
/// | Prefix              | Source                                              | Example                            |
/// |---------------------|-----------------------------------------------------|------------------------------------|
/// | `scheduler:<job>`   | Cron-like recurring jobs in `services::scheduled_jobs` | `scheduler:partition_provisioner` |
/// | `background:<task>` | Fire-and-forget spawns from handlers / services     | `background:notification_delete`   |
/// | `channels:<phase>`  | Channels pipeline (poll, deliver, ack) workers      | `channels:inbound`                 |
/// | `handler:<name>`    | A `PlatformConn` handler overriding its fallback    | `handler:csp_report`               |
/// | `middleware:<name>` | A pre-request middleware that writes audit rows     | `middleware:api_token`             |
/// | `guest:<action>`    | Public unauth guest-token paths                     | `guest:ticket_create`              |
/// | `platform:fallback:<route>` | PlatformConn auto-stamped fallback (means a handler forgot `with_actor`) | `platform:fallback:/api/csp-report` |
/// | `test:<name>`       | Test fixtures and substrate probes                  | `test:background_run_smoke`        |
///
/// Use a stable static string. Don't compose it from runtime data
/// (workspace id, user uuid, ticket id) — that's what the actor's
/// other GUCs are for, and a runtime-composed reference defeats
/// grep-based bucketing. The `&'static str` parameter type is the
/// foreclosure.
///
/// New prefixes are fine as long as they group the same way:
/// a single, descriptive bucket name that tells you *which class
/// of code* emitted the row. Avoid bare nouns like
/// `"plugin_provisioner"` — without a prefix they don't sort or
/// filter cleanly alongside the others.
///
/// # When NOT to use this
///
/// For request-context handler code, use `TenantConn` (or
/// `PlatformConn` for cross-tenant ops). Those extractors do the
/// same acquire-and-wrap dance with a request-bound actor that
/// already carries the user UUID + correlation id; bypassing them
/// in favour of `background_run` loses that attribution.
/// `background_run` is the right answer only when the call site
/// genuinely has no request to attribute to (schedulers, post-
/// response spawn tasks, channel adapters running off their own
/// loops).
pub fn background_run<T>(
    pool: &crate::db::Pool,
    reference: &'static str,
    f: impl FnOnce(&mut DbConnection) -> QueryResult<T>,
) -> Result<T, BackgroundRunError> {
    let mut conn = pool.get().map_err(BackgroundRunError::Pool)?;
    let actor = crate::sync::actor::ActorContext::system(reference);
    with_actor_bypass_context(&mut conn, &actor, f).map_err(BackgroundRunError::Db)
}

/// [`background_run`] pinned to a specific workspace.
///
/// Background writes to tenant tables (notifications, outbound_emails, …)
/// must set `app.workspace_id`: the column default and the audit/sync
/// triggers all read that GUC, so a plain `background_run` (system actor,
/// no workspace) leaves `workspace_id` NULL and the insert fails the NOT
/// NULL constraint. Use this when the caller knows the workspace the write
/// belongs to (a ticket's `workspace_id`, a resolved recipient workspace).
pub fn background_run_in_workspace<T>(
    pool: &crate::db::Pool,
    reference: &'static str,
    workspace_id: i32,
    f: impl FnOnce(&mut DbConnection) -> QueryResult<T>,
) -> Result<T, BackgroundRunError> {
    let mut conn = pool.get().map_err(BackgroundRunError::Pool)?;
    let actor = crate::sync::actor::ActorContext::system(reference).with_workspace(workspace_id);
    with_actor_bypass_context(&mut conn, &actor, f).map_err(BackgroundRunError::Db)
}

/// Like [`background_run_in_workspace`] but NON-bypass: runs the closure as the
/// `nosdesk_app` runtime role (RLS enforced) pinned to `workspace_id`.
///
/// This is the right primitive for per-workspace background work that reads as
/// well as writes tenant tables: the workspace pin satisfies the NOT NULL
/// `workspace_id` column default on inserts AND scopes RLS reads to that one
/// workspace. `background_run`/`background_run_in_workspace` elevate to
/// `nosdesk_admin` (BYPASSRLS), which fixes the write default but leaves an
/// unfiltered read returning an arbitrary tenant's row. Reserve the bypass
/// variants for genuinely cross-workspace operations (e.g. the outbound queue
/// claim that drains every tenant in one pass).
///
/// Used by the channel/notification background paths (auto-ack, notification
/// email delivery, the IMAP poll's per-channel credential read and cursor
/// write), each of which knows its workspace from the channel or ticket.
pub fn run_in_workspace<T>(
    pool: &crate::db::Pool,
    reference: &'static str,
    workspace_id: i32,
    f: impl FnOnce(&mut DbConnection) -> QueryResult<T>,
) -> Result<T, BackgroundRunError> {
    let mut conn = pool.get().map_err(BackgroundRunError::Pool)?;
    let actor = crate::sync::actor::ActorContext::system(reference).with_workspace(workspace_id);
    with_actor_context(&mut conn, &actor, f).map_err(BackgroundRunError::Db)
}

/// Error type returned by `background_run`. Distinguishes "couldn't
/// get a connection from the pool" from "the closure errored".
#[derive(Debug)]
pub enum BackgroundRunError {
    Pool(r2d2::Error),
    Db(diesel::result::Error),
}

impl std::fmt::Display for BackgroundRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pool(e) => write!(f, "pool acquire failed: {e}"),
            Self::Db(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BackgroundRunError {}

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
    fn audited_write_without_workspace_context_raises_named_error() {
        // Layer 2: an audited write that reaches the trigger with no
        // workspace GUC must fail with the typed NDX01 error that names
        // the wrapper, not an opaque NOT NULL violation on a partition.
        let mut conn = setup_test_connection();

        // Elevate to the BYPASSRLS role and clear the workspace pin
        // setup_test_connection installs: bypass skips RLS so the only
        // gate left is the AFTER audit trigger, isolating its check.
        diesel::sql_query("SET LOCAL ROLE nosdesk_admin")
            .execute(&mut conn)
            .expect("set bypass role");
        diesel::sql_query("SELECT set_config('app.workspace_id', '', false) AS set_config")
            .get_result::<DiscardSetConfig>(&mut conn)
            .expect("clear workspace GUC");

        let new_user = crate::models::NewUser {
            uuid: uuid::Uuid::new_v4(),
            name: "no-context".to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        };
        let err = diesel::insert_into(crate::schema::users::table)
            .values(&new_user)
            .execute(&mut conn)
            .expect_err("audited insert without workspace context must fail");

        let msg = err.to_string();
        assert!(msg.contains("audit context missing"), "got: {msg}");
        assert!(msg.contains("with_actor_context"), "got: {msg}");
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

    // ---- Phase 3i.7: background_run substrate coverage ----

    #[test]
    fn background_run_elevates_role_and_seeds_actor_gucs() {
        // background_run is the canonical entry point for any
        // spawn-task DB work. It must (a) acquire a pool
        // connection, (b) elevate to nosdesk_admin so cross-tenant
        // queries bypass RLS, and (c) set the actor GUCs so the
        // audit_log trigger and sync_actions rows attribute the
        // write to the "system:<reference>" actor. Without all
        // three, schedulers silently produce zero-row reads / writes
        // post-3h.4.
        let pool = crate::test_helpers::setup_test_pool();

        background_run(&pool, "test:background_run_smoke", |conn| {
            // BYPASSRLS role active for the txn.
            let current_role: String =
                diesel::sql_query("SELECT current_user::text AS current_setting")
                    .get_result::<GucReadback>(conn)
                    .map(|r| r.current_setting.unwrap_or_default())
                    .unwrap_or_default();
            assert_eq!(
                current_role, "nosdesk_admin",
                "background_run must SET LOCAL ROLE nosdesk_admin"
            );

            assert_eq!(
                read_guc(conn, "app.actor_kind"),
                Some("system".to_string()),
                "system actor kind"
            );
            assert_eq!(
                read_guc(conn, "app.actor_ref"),
                Some("test:background_run_smoke".to_string()),
                "reference label passes through to GUC"
            );

            Ok::<(), diesel::result::Error>(())
        })
        .expect("background_run succeeded");
    }

    #[test]
    fn background_run_propagates_closure_errors() {
        // background_run wraps the closure in a transaction; the
        // closure's error should surface through BackgroundRunError::Db
        // (not silently swallowed) so a scheduled-job tick logs and
        // counts the failure instead of looking like success.
        let pool = crate::test_helpers::setup_test_pool();
        let result: Result<(), BackgroundRunError> =
            background_run(&pool, "test:background_run_err", |_| {
                Err::<(), diesel::result::Error>(diesel::result::Error::NotFound)
            });
        assert!(matches!(
            result,
            Err(BackgroundRunError::Db(diesel::result::Error::NotFound))
        ));
    }
}
