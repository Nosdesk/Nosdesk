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
    // emit, so that event's groups carry the project.
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
    let placeholder_re =
        Regex::new(r#"(?i)\b(insert\s+into|delete\s+from|update)\s+\\?"?\{"#).unwrap();
    let table_res: Vec<(&str, &str, Regex)> = OWNERS
        .iter()
        .map(|&(table, owner)| {
            let re = Regex::new(&format!(
                r"(?i)(insert_into|update|delete)\s*\(\s*(crate::schema::)?{table}(::table|\.|\s*\))|(insert\s+into|update|delete\s+from)\s+(public\.)?{table}\b"
            ))
            .unwrap();
            (table, owner, re)
        })
        .collect();

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
        let src = code_only(&fs::read_to_string(entry.path()).expect("read source"));
        let allowed = |table: &str| ALLOWLIST.contains(&(relpath.as_str(), table));

        for (table, owner, re) in &table_res {
            if relpath == *owner || allowed(table) || allowed("*") {
                continue;
            }
            for m in re.find_iter(&src) {
                stray.push(format!("{relpath}: {table}: {}", m.as_str()));
            }
        }
        if !allowed("*") {
            for m in placeholder_re.find_iter(&src) {
                stray.push(format!("{relpath}: unknown table: {}", m.as_str()));
            }
        }
    }

    assert!(
        stray.is_empty(),
        "ticket join tables written outside their owning repository file; call the owner's \
         functions, which emit, or add an ALLOWLIST entry with a reason:\n  {}",
        stray.join("\n  ")
    );
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
