//! Lint: writes to `tickets` stay in the ticket repository.
//!
//! `repository/tickets.rs` is the one place a ticket is created, changed or
//! deleted: `update_ticket_partial` checks the assignee, records the event
//! clients, notifications and webhooks read, and keeps the SLA pill current.
//! A raw write anywhere else skips all of that, which is how a team step came
//! to assign tickets to requesters. This finds, outside the repository:
//!
//! - Diesel writes whose target is a path ending in `tickets` (`tickets::table`,
//!   `t::tickets` through a `dsl as t` alias, `super::schema::tickets::table`,
//!   `crate::schema::tickets::table`), across line breaks;
//! - writes through a reference to a ticket (`update(&ticket)`) and through a
//!   query or row bound from `tickets` first (`let q = tickets::table...;`
//!   then `delete(q)`);
//! - SQL that inserts into, updates or deletes from `tickets`, in any case,
//!   with `ONLY`, a schema or an alias (`UPDATE tickets t SET`).
//!
//! Test code doesn't count: `#[cfg(test)]` items and modules, including a
//! module declared `#[cfg(test)] mod name;` in its own file.
//!
//! Out of its reach: the restores that copy every table generically, with the
//! table name in a variable (`services/workspace_import.rs` and
//! `services/backup.rs` build `INSERT INTO "{table}"`). They put back rows as
//! they were saved, history rather than edits, so the ticket write path's
//! checks and events don't apply to them.
//!
//! ## Escape hatch
//!
//! A file that writes `tickets` for a reason of its own goes in
//! `WRITE_ALLOWLIST` with the reason.

#![allow(clippy::expect_used)]

use std::collections::HashSet;

use regex::Regex;

use crate::lint_source::{mask, rust_sources, test_only_files, test_regions};

const OWNER: &str = "repository/tickets.rs";

/// Files outside the ticket repository that write `tickets`, and why.
const WRITE_ALLOWLIST: &[(&str, &str)] = &[
    (
        "services/sla.rs",
        "stamps the SLA targets and clock it owns, and emits their change",
    ),
    (
        "services/scheduled_jobs.rs",
        "the breach scan stamps the SLA breach columns it owns",
    ),
    (
        "repository/ticket_approvals.rs",
        "sets `approval_state`, which the approval round owns",
    ),
    (
        "repository/comments.rs",
        "a reply stamps `first_response_at` and bumps `updated_at`",
    ),
    (
        "repository/article_content.rs",
        "saving a ticket's article bumps its `updated_at`",
    ),
    (
        "repository/ticket_merge.rs",
        "a merge moves its sources to the merged state with a raw update, since \
         `update_ticket_partial` would send a status-change notification, then sends \
         `ticket.merged_into` with each source's whole row",
    ),
    ("bin/seed_demo.rs", "demo data, backdated by hand"),
];

#[test]
fn writes_to_tickets_stay_in_the_ticket_repository() {
    let sources = rust_sources();
    let test_files = test_only_files(&sources);

    let mut stray = Vec::new();
    let mut allowlisted_seen = HashSet::new();
    for (rel, src) in &sources {
        if rel == "schema.rs" || rel == OWNER || test_files.contains(rel) {
            continue;
        }
        let writes = ticket_writes(src);
        if writes.is_empty() {
            continue;
        }
        if WRITE_ALLOWLIST.iter().any(|(file, _)| file == rel) {
            allowlisted_seen.insert(rel.clone());
            continue;
        }
        for (line, text) in writes {
            stray.push(format!("{rel}:{line}: {text}"));
        }
    }

    assert!(
        stray.is_empty(),
        "writes to `tickets` outside {OWNER}; go through its functions (update_ticket_partial \
         for a change), or add the file to WRITE_ALLOWLIST with a reason:\n  {}",
        stray.join("\n  ")
    );
    let stale: Vec<&str> = WRITE_ALLOWLIST
        .iter()
        .map(|(file, _)| *file)
        .filter(|file| !allowlisted_seen.contains(*file))
        .collect();
    assert!(
        stale.is_empty(),
        "WRITE_ALLOWLIST names files that no longer write `tickets`; remove them: {stale:?}"
    );
}

/// Each spelling the lint must catch, and the test code it must not.
#[test]
fn the_lint_sees_every_spelling_and_skips_test_code() {
    let caught = [
        "diesel::update(tickets::table.find(id)).set(x).execute(conn)",
        "diesel::update(crate::schema::tickets::table.find(id))",
        "diesel::insert_into(schema::tickets::table).values(&t)",
        "diesel::delete(\n        tickets::table\n            .filter(f))",
        "diesel::update(tickets.find(id))",
        r#"diesel::sql_query("UPDATE tickets SET title = $1")"#,
        r#"diesel::sql_query("insert into public.tickets (title) values ($1)")"#,
        r#"sql_query("DELETE FROM tickets WHERE id = $1")"#,
        "diesel::update(t::tickets.find(id))",
        "diesel::update(super::schema::tickets::table.find(id))",
        "diesel::update(&ticket).set(x)",
        "diesel::delete(&self.ticket)",
        "let q = tickets::table.filter(f);\n    diesel::delete(q).execute(c)",
        "let mut doomed: Q = t::tickets.filter(f).into_boxed();\n    diesel::update(doomed)",
        r#"sql_query("UPDATE tickets t SET title = $1")"#,
        r#"sql_query("UPDATE ONLY tickets SET title = $1")"#,
        r#"sql_query("update public.tickets as t set title = $1")"#,
    ];
    for code in caught {
        assert_eq!(ticket_writes(code).len(), 1, "should catch: {code}");
    }
    let ignored = [
        "tickets::table.find(id).first(conn)",
        "diesel::update(ticket_assets::table)",
        "diesel::update(linked_tickets::table)",
        "diesel::update(t::ticket_assets.find(id))",
        "diesel::update(&ticket_asset)",
        "let q = ticket_assets::table.filter(f);\n    diesel::delete(q)",
        "// diesel::update(tickets::table) in a comment",
        "/* UPDATE tickets SET x */ let a = 1;",
        r#"warn!("failed to update tickets for {id}")"#,
        "#[cfg(test)]\nmod tests {\n    fn f() { diesel::update(tickets::table).execute(c); }\n}",
        "#[cfg(test)]\nfn helper(c: &mut Conn) {\n    diesel::update(tickets::table);\n}",
    ];
    for code in ignored {
        assert!(ticket_writes(code).is_empty(), "should ignore: {code}");
    }
    // After a test module, code counts again.
    let after = "#[cfg(test)]\nmod tests { fn f() { let s = \"}\"; } }\nfn g() { diesel::update(tickets::table); }";
    assert_eq!(ticket_writes(after).len(), 1);
}

/// The writes to `tickets` in `src` outside test code, as (line, text).
fn ticket_writes(src: &str) -> Vec<(usize, String)> {
    let code = mask(src, true);
    let sql = mask(src, false);
    let skipped = test_regions(&code);
    // A write whose target is a path ending in `tickets`.
    let diesel = Regex::new(r"\b(?:insert_into|update|delete)\s*\(\s*(?:\w+\s*::\s*)*tickets\b")
        .expect("diesel write pattern");
    // A write through a reference to a ticket row.
    let by_ref = Regex::new(r"\b(?:update|delete)\s*\(\s*&\s*(?:\w+\s*\.\s*)*\w*ticket\b")
        .expect("reference write pattern");
    // A query or row bound from `tickets`, written through later.
    let bound = Regex::new(r"\blet\s+(?:mut\s+)?(\w+)\s*(?::[^=;]*)?=\s*(?:\w+\s*::\s*)*tickets\b")
        .expect("binding pattern");
    let through: Vec<Regex> = bound
        .captures_iter(&code)
        .map(|caps| {
            Regex::new(&format!(
                r"\b(?:update|delete)\s*\(\s*&?\s*{}\b",
                regex::escape(&caps[1])
            ))
            .expect("bound write pattern")
        })
        .collect();
    let raw = Regex::new(
        r#"(?i)\b(?:update\s+(?:only\s+)?(?:public\.)?"?tickets"?(?:\s+(?:as\s+)?\w+)?\s+set|insert\s+into\s+(?:public\.)?"?tickets"?\b|delete\s+from\s+(?:only\s+)?(?:public\.)?"?tickets"?\b)"#,
    )
    .expect("sql write pattern");
    let mut found: Vec<(usize, String)> = diesel
        .find_iter(&code)
        .chain(by_ref.find_iter(&code))
        .chain(through.iter().flat_map(|re| re.find_iter(&code)))
        .chain(raw.find_iter(&sql))
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
