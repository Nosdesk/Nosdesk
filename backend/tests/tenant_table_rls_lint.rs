//! Lint: every tenant table (one with a `workspace_id` column) must have
//! row-level security ENABLED and FORCED, plus at least one policy.
//!
//! Workspace isolation is enforced by an RLS policy of the form
//!   USING / WITH CHECK (workspace_id = current_setting('app.workspace_id'))
//! on every tenant table, with FORCE so it applies to the table owner too.
//! A tenant table that ships without it relies solely on each query
//! remembering its `WHERE workspace_id = ...` clause — one omission leaks or
//! clobbers across workspaces, with no database backstop. ENABLE without a
//! policy is the opposite failure: it denies all rows. FORCE matters because
//! the tables are owned by a role that would otherwise bypass RLS.
//!
//! This caught `workspace_members`, the lone tenant table that initially
//! shipped with no RLS at all; it now carries the standard ENABLE/FORCE plus
//! isolation policy inline in the initial schema.
//! Sibling to `tenant_table_grants_lint` (which guards the DML grants).
//!
//! ## Escape hatch
//!
//! A table that carries `workspace_id` but is deliberately NOT workspace-
//! isolated by RLS goes in `RLS_EXEMPT_ALLOWLIST` WITH a one-line reason.
//! The list is empty today: every workspace_id table is RLS-protected.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Text};

mod common;
use common::TestDb;

/// Tables that have a `workspace_id` column but intentionally do not carry
/// the workspace-isolation RLS policy. Add an entry only with a
/// justification, e.g. `"some_global_table", // platform-scoped, not tenant`.
const RLS_EXEMPT_ALLOWLIST: &[&str] = &[];

#[derive(QueryableByName, Debug)]
struct TableRls {
    #[diesel(sql_type = Text)]
    table_name: String,
    #[diesel(sql_type = Bool)]
    enabled: bool,
    #[diesel(sql_type = Bool)]
    forced: bool,
    #[diesel(sql_type = BigInt)]
    policies: i64,
}

#[test]
fn tenant_tables_have_forced_rls_with_a_policy() {
    let db = TestDb::new();
    let mut conn = db.conn();

    // One row per base table that carries a workspace_id column, with its RLS
    // enabled/forced flags and policy count.
    let rows: Vec<TableRls> = diesel::sql_query(
        "SELECT c.relname AS table_name, \
                c.relrowsecurity AS enabled, \
                c.relforcerowsecurity AS forced, \
                (SELECT count(*) FROM pg_policy p WHERE p.polrelid = c.oid) AS policies \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'public' \
           AND c.relkind = 'r' \
           AND EXISTS ( \
             SELECT 1 FROM information_schema.columns col \
             WHERE col.table_schema = 'public' AND col.table_name = c.relname \
               AND col.column_name = 'workspace_id' \
           )",
    )
    .load(&mut conn)
    .expect("query pg_class RLS flags for workspace_id tables");

    assert!(
        !rows.is_empty(),
        "no workspace_id tables found — the test DB is not migrated as expected"
    );

    let violations: Vec<String> = rows
        .into_iter()
        .filter(|r| !(r.enabled && r.forced && r.policies > 0))
        .filter(|r| !RLS_EXEMPT_ALLOWLIST.contains(&r.table_name.as_str()))
        .map(|r| {
            let mut missing = Vec::new();
            if !r.enabled {
                missing.push("ENABLE ROW LEVEL SECURITY".to_string());
            }
            if !r.forced {
                missing.push("FORCE ROW LEVEL SECURITY".to_string());
            }
            if r.policies == 0 {
                missing.push("an isolation policy".to_string());
            }
            format!("{} (missing {})", r.table_name, missing.join(" + "))
        })
        .collect();

    assert!(
        violations.is_empty(),
        "these tables carry workspace_id but are not fully RLS-protected, so \
         workspace isolation rests entirely on per-query WHERE clauses. Add the \
         standard policy (see the workspace_members RLS in the initial schema for the \
         form), or add the table to RLS_EXEMPT_ALLOWLIST with a justification:\n  {}",
        violations.join("\n  ")
    );
}
