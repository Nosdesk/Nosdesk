//! Lint: a foreign key between two workspace tables includes the workspace.
//!
//! Row security scopes reads to one workspace, but Postgres checks foreign keys
//! without it. A key from one workspace table to another is therefore
//! two-column, `(workspace_id, x_id)` referencing the parent's
//! `(workspace_id, id)`, so the database itself keeps a row's references in its
//! own workspace, whichever connection writes it. Keys to `workspaces` itself
//! are covered by `workspace_fk_cascade_lint`.
//!
//! ## Escape hatch
//!
//! A key that intentionally references another workspace's row goes in
//! `UNSCOPED_ALLOWLIST`, keyed by constraint name, with a reason.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};

use crate::common::TestDb;

/// Keys between workspace tables that intentionally leave out the workspace.
const UNSCOPED_ALLOWLIST: &[&str] = &[
    // (empty): every key between workspace tables includes the workspace.
];

#[derive(QueryableByName, Debug)]
struct UnscopedFk {
    #[diesel(sql_type = Text)]
    table_name: String,
    #[diesel(sql_type = Text)]
    constraint_name: String,
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// Keys between two tables that both have `workspace_id`, other than keys to
/// `workspaces`.
const BETWEEN_WORKSPACE_TABLES: &str = "con.contype = 'f' \
     AND con.connamespace = 'public'::regnamespace \
     AND con.confrelid <> 'public.workspaces'::regclass \
     AND EXISTS (SELECT 1 FROM pg_attribute a \
                 WHERE a.attrelid = con.conrelid AND a.attname = 'workspace_id' \
                   AND NOT a.attisdropped) \
     AND EXISTS (SELECT 1 FROM pg_attribute a \
                 WHERE a.attrelid = con.confrelid AND a.attname = 'workspace_id' \
                   AND NOT a.attisdropped)";

/// The key pairs the child's `workspace_id` with the parent's.
const PAIRS_THE_WORKSPACE: &str =
    "EXISTS (SELECT 1 FROM unnest(con.conkey, con.confkey) AS k(c, p) \
     JOIN pg_attribute ca ON ca.attrelid = con.conrelid AND ca.attnum = k.c \
     JOIN pg_attribute pa ON pa.attrelid = con.confrelid AND pa.attnum = k.p \
     WHERE ca.attname = 'workspace_id' AND pa.attname = 'workspace_id')";

#[test]
fn every_key_between_workspace_tables_includes_the_workspace() {
    let db = TestDb::new();
    let mut conn = db.conn();

    let scoped = diesel::sql_query(format!(
        "SELECT count(*) AS n FROM pg_constraint con \
         WHERE {BETWEEN_WORKSPACE_TABLES} AND {PAIRS_THE_WORKSPACE}"
    ))
    .get_result::<Count>(&mut conn)
    .expect("count workspace-scoped keys");
    assert!(
        scoped.n > 0,
        "no workspace-scoped foreign keys found: the test DB is not migrated as expected"
    );

    let rows: Vec<UnscopedFk> = diesel::sql_query(format!(
        "SELECT con.conrelid::regclass::text AS table_name, con.conname AS constraint_name \
         FROM pg_constraint con \
         WHERE {BETWEEN_WORKSPACE_TABLES} AND NOT {PAIRS_THE_WORKSPACE} \
         ORDER BY 1, 2"
    ))
    .load(&mut conn)
    .expect("query pg_constraint for keys between workspace tables");

    let violations: Vec<String> = rows
        .into_iter()
        .filter(|r| !UNSCOPED_ALLOWLIST.contains(&r.constraint_name.as_str()))
        .map(|r| format!("{} ({})", r.table_name, r.constraint_name))
        .collect();
    assert!(
        violations.is_empty(),
        "these foreign keys between workspace tables leave out the workspace, so a \
         row could reference another workspace's row. Make each \
         `FOREIGN KEY (workspace_id, x_id) REFERENCES parent (workspace_id, id)` \
         (the parent needs a unique (workspace_id, id) index), add a joinable! line to \
         src/schema_joins.rs if code joins across it, or add the constraint to \
         UNSCOPED_ALLOWLIST with a reason:\n  {}",
        violations.join("\n  ")
    );
}
