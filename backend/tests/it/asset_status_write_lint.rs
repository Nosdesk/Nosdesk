//! Lint: `assets.status` is written only where the status rules are asked.
//!
//! Every move of an asset's status goes through
//! `services::assets::status_rules`: `asset_lifecycle::transition`, the loan
//! ledger's `asset_lifecycle::move_for_loan` and `asset_lifecycle::create`.
//! Their one write is the private `set_status` in
//! `repository/asset_lifecycle.rs`, plus the insert that creates an asset in
//! a status of its own in `repository/assets.rs`. A new writer anywhere else
//! would skip the matrix, which is how the loan ledger once came to set a
//! status without the row lock. This finds, outside test code:
//!
//! - Diesel's `assets::status.eq(..)` (a `.set(..)` or an insert tuple), but
//!   not one inside `.filter(..)`, which reads;
//! - SQL that updates `assets` and sets `status`, or inserts into `assets`
//!   naming `status`.
//!
//! Out of its reach, as for the other table lints: the restores that copy
//! every table generically, which put rows back as they were saved.
//!
//! ## Escape hatch
//!
//! `WRITERS` lists each file that writes the column and how many writes it
//! has, with the reason. A count that changes fails the lint, so a second
//! writer in a listed file is seen too.

#![allow(clippy::expect_used)]

use regex::Regex;

use crate::lint_source::{mask, rust_sources, test_only_files, test_regions};

/// The files that write `assets.status`, how many writes each has, and why.
const WRITERS: &[(&str, usize, &str)] = &[
    (
        "repository/asset_lifecycle.rs",
        1,
        "`set_status`, called only by the moves that asked the status rules",
    ),
    (
        "repository/assets.rs",
        1,
        "`create_device_in` inserts an asset in a status `asset_lifecycle::create` checked",
    ),
];

/// The writes of `assets.status` in `src`, outside test code, as (line, text).
fn status_writes(src: &str) -> Vec<(usize, String)> {
    let code = mask(src, true);
    let sql = mask(src, false);
    let skipped = test_regions(&code);
    let diesel = Regex::new(r"\bassets\s*::\s*status\s*\.\s*eq\s*\(").expect("diesel pattern");
    let filter = Regex::new(r"\.\s*filter\s*\(\s*$").expect("filter pattern");
    // An UPDATE's SET list (up to its WHERE) or an INSERT's column list.
    let update = Regex::new(
        r#"(?is)\bupdate\s+(?:only\s+)?(?:public\.)?"?assets"?(?:\s+(?:as\s+)?\w+)?\s+set\b([^;"]*)"#,
    )
    .expect("update pattern");
    let insert = Regex::new(r#"(?is)\binsert\s+into\s+(?:public\.)?"?assets"?\s*\(([^)]*)\)"#)
        .expect("insert pattern");
    let where_clause = Regex::new(r"(?is)\bwhere\b.*").expect("where pattern");
    let sets_status = Regex::new(r"(?i)\bstatus\s*=").expect("set pattern");
    let names_status = Regex::new(r"(?i)\bstatus\b").expect("column pattern");
    let sql_writes = update
        .captures_iter(&sql)
        .filter(|c| sets_status.is_match(&where_clause.replace(&c[1], "")))
        .chain(
            insert
                .captures_iter(&sql)
                .filter(|c| names_status.is_match(&c[1])),
        )
        .filter_map(|c| c.get(0));
    let mut found: Vec<(usize, String)> = diesel
        .find_iter(&code)
        .filter(|m| !filter.is_match(&code[..m.start()]))
        .chain(sql_writes)
        .filter(|m| !skipped.iter().any(|r| r.contains(&m.start())))
        .map(|m| {
            let line = src[..m.start()].matches('\n').count() + 1;
            let text = src[m.range()]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            (line, text)
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

#[test]
fn asset_status_is_written_only_behind_the_status_rules() {
    let sources = rust_sources();
    let test_files = test_only_files(&sources);

    let mut stray = Vec::new();
    for (rel, src) in &sources {
        if rel == "schema.rs" || test_files.contains(rel) {
            continue;
        }
        let writes = status_writes(src);
        let expected = WRITERS
            .iter()
            .find(|(file, _, _)| file == rel)
            .map_or(0, |(_, count, _)| *count);
        if writes.len() != expected {
            stray.push(format!(
                "{rel}: {} writes of assets.status, {expected} expected:\n    {}",
                writes.len(),
                writes
                    .iter()
                    .map(|(line, text)| format!("{line}: {text}"))
                    .collect::<Vec<_>>()
                    .join("\n    ")
            ));
        }
    }
    for (file, _, _) in WRITERS {
        assert!(
            sources.iter().any(|(rel, _)| rel == file),
            "WRITERS names {file}, which doesn't exist"
        );
    }
    assert!(
        stray.is_empty(),
        "assets.status is written outside the status rules; move the asset through \
         asset_lifecycle (transition, move_for_loan, create), or list the file in WRITERS \
         with a reason:\n  {}",
        stray.join("\n  ")
    );
}

/// Each spelling the lint must catch, and the code it must not.
#[test]
fn the_lint_sees_every_spelling_and_skips_reads_and_test_code() {
    let caught = [
        "diesel::update(assets::table.find(id)).set(assets::status.eq(\"lost\"))",
        "diesel::update(t).set((assets::status.eq(s), assets::updated_at.eq(now)))",
        "diesel::insert_into(assets::table).values((&new, assets::status.eq(s)))",
        "crate::schema::assets::status.eq(s)",
        r#"diesel::sql_query("UPDATE assets SET status = $1 WHERE id = $2")"#,
        r#"sql_query("update public.assets a set updated_at = now(), status = 'lost'")"#,
        r#"sql_query("INSERT INTO assets (name, kind, status) VALUES ($1, $2, $3)")"#,
    ];
    for code in caught {
        assert_eq!(status_writes(code).len(), 1, "should catch: {code}");
    }
    let ignored = [
        "assets::table.filter(assets::status.eq(\"on_loan\")).load(conn)",
        "query.filter(assets::status.eq_any(values))",
        "query.order(assets::status.asc())",
        "diesel::update(tickets::table).set(tickets::status.eq(s))",
        r#"sql_query("UPDATE assets SET name = $1 WHERE status = 'lost'")"#,
        r#"sql_query("SELECT status FROM assets")"#,
        "// assets::status.eq(s) in a comment",
        "#[cfg(test)]\nmod tests {\n    fn f() { diesel::update(t).set(assets::status.eq(s)); }\n}",
    ];
    for code in ignored {
        assert!(status_writes(code).is_empty(), "should ignore: {code}");
    }
}
