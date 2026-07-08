//! Lint: every foreign key that references `workspaces` must be
//! `ON DELETE CASCADE`.
//!
//! Workspace hard-delete (GDPR erase, region migration, tenant offboarding)
//! relies on the FK cascade to purge every tenant row: the delete hits
//! `workspaces` and Postgres fans the delete out through each referencing
//! table. A new tenant table whose `workspace_id` FK is `NO ACTION` / `RESTRICT`
//! would make the workspace delete fail (the FK blocks it) or, worse if it were
//! `SET NULL`, silently orphan rows with a NULL `workspace_id` that then escape
//! RLS scoping. The cascade was originally stitched together in a one-time
//! DO-block, so a table added later can quietly miss it; this asserts the
//! invariant against the live schema instead.
//!
//! ## Escape hatch
//!
//! A FK to `workspaces` that intentionally is NOT cascade goes in
//! `NON_CASCADE_ALLOWLIST` keyed by constraint name, WITH a one-line reason.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::Text;

mod common;
use common::TestDb;

/// Constraints referencing `workspaces` that intentionally are not
/// `ON DELETE CASCADE`. Add an entry only with a justification.
const NON_CASCADE_ALLOWLIST: &[&str] = &[
    // (empty) — every workspaces FK cascades so hard-delete purges cleanly.
];

#[derive(QueryableByName, Debug)]
struct WorkspaceFk {
    #[diesel(sql_type = Text)]
    table_name: String,
    #[diesel(sql_type = Text)]
    constraint_name: String,
    #[diesel(sql_type = Text)]
    delete_action: String,
}

#[test]
fn every_workspaces_fk_is_on_delete_cascade() {
    let db = TestDb::new();
    let mut conn = db.conn();

    // One row per FK constraint whose referenced table is `workspaces`.
    // `confdeltype` is Postgres's single-char delete action: 'c' = CASCADE,
    // 'a' = NO ACTION, 'r' = RESTRICT, 'n' = SET NULL, 'd' = SET DEFAULT.
    let rows: Vec<WorkspaceFk> = diesel::sql_query(
        "SELECT con.conrelid::regclass::text AS table_name, \
                con.conname AS constraint_name, \
                con.confdeltype::text AS delete_action \
         FROM pg_constraint con \
         JOIN pg_class ref ON ref.oid = con.confrelid \
         JOIN pg_namespace ns ON ns.oid = ref.relnamespace \
         WHERE con.contype = 'f' \
           AND ref.relname = 'workspaces' \
           AND ns.nspname = 'public'",
    )
    .load(&mut conn)
    .expect("query pg_constraint for workspaces FKs");

    assert!(
        !rows.is_empty(),
        "no foreign keys to `workspaces` found — the test DB is not migrated \
         as expected"
    );

    let violations: Vec<String> = rows
        .into_iter()
        .filter(|r| r.delete_action != "c")
        .filter(|r| !NON_CASCADE_ALLOWLIST.contains(&r.constraint_name.as_str()))
        .map(|r| {
            let action = match r.delete_action.as_str() {
                "a" => "NO ACTION",
                "r" => "RESTRICT",
                "n" => "SET NULL",
                "d" => "SET DEFAULT",
                other => other,
            };
            format!("{} ({} is {})", r.table_name, r.constraint_name, action)
        })
        .collect();

    assert!(
        violations.is_empty(),
        "these foreign keys to `workspaces` are not ON DELETE CASCADE, so a \
         workspace hard-delete will fail or orphan rows. Make the FK \
         `ON DELETE CASCADE`, or add the constraint to NON_CASCADE_ALLOWLIST \
         with a justification:\n  {}",
        violations.join("\n  ")
    );
}
