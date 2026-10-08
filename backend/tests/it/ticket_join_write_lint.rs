//! Lint: each ticket join table is written only by its owning repository file.
//!
//! The owners' functions emit the sync events (and so the webhooks) that keep
//! every client's boards, sidebars and link lists current. A merge that
//! rewrote these tables with raw SQL moved projects, assets, tags, watchers
//! and links without telling anyone, so the moves only showed after a reload.
//!
//! ## Escape hatch
//!
//! A write that can't go through the owner goes in `ALLOWLIST` with a one-line
//! reason. A raw statement whose table is a format placeholder can't be
//! attributed to a table, so it needs an entry too.

#![allow(clippy::expect_used)]

use regex::Regex;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

/// Each ticket join table and the file that owns its writes.
const OWNERS: &[(&str, &str)] = &[
    ("project_tickets", "repository/projects.rs"),
    ("cycle_tickets", "repository/cycles.rs"),
    ("ticket_assets", "repository/tickets.rs"),
    ("ticket_tags", "repository/tags.rs"),
    ("ticket_watchers", "repository/ticket_watchers.rs"),
    ("linked_tickets", "repository/linked_tickets.rs"),
    (
        "documentation_page_tickets",
        "repository/documentation_page_tickets.rs",
    ),
];

/// Writes outside the owner, by (file, table). `"*"` is a placeholder table.
const ALLOWLIST: &[(&str, &str)] = &[
    // Creating a ticket in a project links it before the `ticket.created`
    // emit, so that event's groups carry the project; deleting a ticket
    // takes it off its projects with it.
    ("repository/tickets.rs", "project_tickets"),
    // Deleting a ticket deletes its links with it.
    ("repository/tickets.rs", "linked_tickets"),
    // The demo seeder backdates memberships it made through the owner.
    ("bin/seed_demo.rs", "cycle_tickets"),
    // Account deletion clears `created_by`, which no sync event carries.
    ("repository/users.rs", "project_tickets"),
    ("repository/users.rs", "ticket_assets"),
    ("repository/users.rs", "linked_tickets"),
    // Workspace import writes every exported table's rows back as they were.
    ("services/workspace_import.rs", "*"),
];

#[test]
fn ticket_join_tables_are_written_by_their_owners() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stray = Vec::new();
    for entry in WalkDir::new(&src_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
    {
        let relpath = entry
            .path()
            .strip_prefix(&src_root)
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");
        if relpath == "schema.rs" || relpath == "test_helpers.rs" {
            continue;
        }
        let src = fs::read_to_string(entry.path()).expect("read source");
        stray.extend(stray_writes(&relpath, &src));
    }

    assert!(
        stray.is_empty(),
        "ticket join tables written outside their owning repository file; call the owner's \
         functions, which emit, or add an ALLOWLIST entry with a reason:\n  {}",
        stray.join("\n  ")
    );
}

#[test]
fn every_spelling_of_a_write_is_caught() {
    let caught = |src: &str| stray_writes("repository/elsewhere.rs", src).len();
    for write in [
        "diesel::insert_into(crate::schema::ticket_watchers::table).values(&row)",
        "diesel::delete(schema::ticket_watchers::table.filter(x))",
        "diesel::insert_into(w::ticket_watchers).values(&row)",
        "diesel::delete(ticket_watchers.filter(x))",
        r#"sql_query("INSERT INTO ticket_watchers (ticket_id) VALUES ($1)")"#,
        r#"format!("DELETE FROM {table} WHERE ticket_id = ANY($1)")"#,
    ] {
        assert_eq!(caught(write), 1, "{write}");
    }
    let aliased = "use crate::schema::project_tickets as p;\n\
                   diesel::insert_into(p::table).values(&row)";
    assert_eq!(caught(aliased), 1);
    let dsl = "use crate::schema::cycle_tickets::dsl as c;\n\
               diesel::update(c::cycle_tickets.filter(x))";
    assert!(caught(dsl) >= 1);
    // Reads and the owner's own writes are fine.
    assert_eq!(caught("ticket_watchers::table.filter(x).load(conn)"), 0);
    assert!(stray_writes(
        "repository/ticket_watchers.rs",
        "diesel::insert_into(ticket_watchers::table).values(&row)"
    )
    .is_empty());
}

/// Writes in `src` (from the file at `relpath` under `src/`) to a ticket join
/// table it doesn't own and isn't allowed.
fn stray_writes(relpath: &str, src: &str) -> Vec<String> {
    let src = code_only(src);
    let allowed = |table: &str| ALLOWLIST.contains(&(relpath, table));
    let mut stray = Vec::new();
    for &(table, owner) in OWNERS {
        if relpath == owner || allowed(table) || allowed("*") {
            continue;
        }
        // Any path to the table (`crate::schema::T::table`, `schema::T`, a
        // dsl's `w::T`), or a raw statement naming it.
        let direct = Regex::new(&format!(
            r"(?i)(insert_into|update|delete)\s*\(\s*(\w+::)*{table}(::table|\.|\s*\))|(insert\s+into|update|delete\s+from)\s+(public\.)?{table}\b"
        ))
        .unwrap();
        for m in direct.find_iter(&src) {
            stray.push(format!("{relpath}: {table}: {}", m.as_str()));
        }
        // A module alias (`use crate::schema::T as p;`, `T::dsl as w`) names
        // the table only at its `use`.
        let alias = Regex::new(&format!(r"\b{table}(?:::dsl)?\s+as\s+(\w+)\b")).unwrap();
        for name in alias.captures_iter(&src).map(|c| c[1].to_string()) {
            let aliased = Regex::new(&format!(
                r"(insert_into|update|delete)\s*\(\s*{name}::(table|{table})\b"
            ))
            .unwrap();
            for m in aliased.find_iter(&src) {
                stray.push(format!("{relpath}: {table} as {name}: {}", m.as_str()));
            }
        }
    }
    if !allowed("*") {
        let placeholder =
            Regex::new(r#"(?i)\b(insert\s+into|delete\s+from|update)\s+\\?"?\{"#).unwrap();
        for m in placeholder.find_iter(&src) {
            stray.push(format!("{relpath}: unknown table: {}", m.as_str()));
        }
    }
    stray
}

/// The source without `#[cfg(test)]` modules, block comments or `//` comment
/// lines, so fixtures and prose don't count as writes.
fn code_only(src: &str) -> String {
    let mut out = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0usize;
    let mut in_block = false;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        if in_block {
            if trimmed.contains("*/") {
                in_block = false;
            }
            i += 1;
            continue;
        }
        if trimmed.starts_with("/*") {
            in_block = !trimmed.contains("*/");
            i += 1;
            continue;
        }
        if trimmed.starts_with("//") {
            i += 1;
            continue;
        }
        if trimmed.starts_with("#[cfg(test)]") {
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim().is_empty() {
                j += 1;
            }
            let next = lines.get(j).map(|l| l.trim_start()).unwrap_or("");
            if next.starts_with("mod ") || next.starts_with("pub mod ") {
                let mut depth = 0i32;
                let mut started = false;
                let mut k = j;
                while k < lines.len() {
                    for b in lines[k].bytes() {
                        match b {
                            b'{' => {
                                depth += 1;
                                started = true;
                            }
                            b'}' => depth -= 1,
                            _ => {}
                        }
                    }
                    k += 1;
                    if started && depth == 0 {
                        break;
                    }
                }
                i = k;
                continue;
            }
        }
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n")
}
