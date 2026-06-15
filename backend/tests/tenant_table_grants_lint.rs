//! Lint: every table the application role (`nosdesk_app`) can INSERT into
//! must also be granted UPDATE and DELETE.
//!
//! Writes run as the RLS-enforced `nosdesk_app` role (via
//! `with_actor_context` / `SET LOCAL ROLE nosdesk_app`). A table granted
//! only `SELECT,INSERT` looks fine until the first UPDATE/DELETE reaches
//! it, then fails at runtime with `permission denied for table <t>` — a
//! 500 that no compile or type check catches. This is a GRANT failure,
//! distinct from RLS (which denies rows, not the whole statement).
//!
//! This caught `workspace_members`, which initially shipped with only
//! `SELECT,INSERT` (the lone exception among 100+ tenant tables) and so
//! 500'd the W2 role-change UPDATE. It now carries the full DML grant
//! inline in the initial schema like every other tenant table.
//!
//! ## Escape hatch
//!
//! A genuinely append-only table (INSERT but never UPDATE/DELETE by the
//! app role) goes in `APPEND_ONLY_ALLOWLIST` WITH a one-line reason. The
//! list is empty today: no such table exists, every insertable table has
//! full DML.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::{Bool, Text};

mod common;
use common::TestDb;

/// Tables `nosdesk_app` may INSERT into but intentionally must NOT
/// UPDATE or DELETE. Add an entry only with a justification, e.g.
/// `"audit_log", // immutable audit trail, append-only by design`.
const APPEND_ONLY_ALLOWLIST: &[&str] = &[];

#[derive(QueryableByName, Debug)]
struct TableGrant {
    #[diesel(sql_type = Text)]
    table_name: String,
    #[diesel(sql_type = Bool)]
    has_update: bool,
    #[diesel(sql_type = Bool)]
    has_delete: bool,
}

#[test]
fn insertable_tenant_tables_grant_full_dml_to_nosdesk_app() {
    let db = TestDb::new();
    let mut conn = db.conn();

    // One row per base table nosdesk_app can INSERT into, flagging whether
    // it also holds UPDATE / DELETE. Views are excluded (their grants are
    // a separate concern and never carry an app INSERT here).
    let rows: Vec<TableGrant> = diesel::sql_query(
        "SELECT g.table_name AS table_name, \
                bool_or(g.privilege_type = 'UPDATE') AS has_update, \
                bool_or(g.privilege_type = 'DELETE') AS has_delete \
         FROM information_schema.role_table_grants g \
         JOIN information_schema.tables t \
           ON t.table_schema = g.table_schema \
          AND t.table_name = g.table_name \
         WHERE g.grantee = 'nosdesk_app' \
           AND g.table_schema = 'public' \
           AND t.table_type = 'BASE TABLE' \
         GROUP BY g.table_name \
         HAVING bool_or(g.privilege_type = 'INSERT')",
    )
    .load(&mut conn)
    .expect("query information_schema.role_table_grants");

    assert!(
        !rows.is_empty(),
        "no INSERT grants found for nosdesk_app — the test DB is not \
         migrated as expected"
    );

    let violations: Vec<String> = rows
        .into_iter()
        .filter(|r| !(r.has_update && r.has_delete))
        .filter(|r| !APPEND_ONLY_ALLOWLIST.contains(&r.table_name.as_str()))
        .map(|r| {
            let mut missing = Vec::new();
            if !r.has_update {
                missing.push("UPDATE");
            }
            if !r.has_delete {
                missing.push("DELETE");
            }
            format!("{} (missing {})", r.table_name, missing.join("+"))
        })
        .collect();

    assert!(
        violations.is_empty(),
        "nosdesk_app can INSERT into these tables but lacks UPDATE/DELETE, \
         so app writes to them fail at runtime with \"permission denied for \
         table ...\". Grant the missing DML (see the workspace_members grant \
         in the initial schema for the form), or add \
         the table to APPEND_ONLY_ALLOWLIST with a justification:\n  {}",
        violations.join("\n  ")
    );
}
