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
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use regex::Regex;
use walkdir::WalkDir;

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
        "known debt: a merge moves its sources with a raw update",
    ),
    (
        "repository/users.rs",
        "known debt: account purge clears a user's references without an event",
    ),
    ("bin/seed_demo.rs", "demo data, backdated by hand"),
];

#[test]
fn writes_to_tickets_stay_in_the_ticket_repository() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let sources: Vec<(String, String)> = WalkDir::new(&src_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
        .map(|e| {
            let rel = relative(&src_root, e.path());
            (rel, fs::read_to_string(e.path()).expect("read source"))
        })
        .collect();
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

/// `src` with comments blanked, and string and char literal contents blanked
/// too when `strings` is set. Byte offsets and line breaks are kept, so a match
/// in the result points at the same place in `src`.
fn mask(src: &str, strings: bool) -> String {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let blank = |out: &mut Vec<u8>, range: Range<usize>| {
        for i in range {
            if out[i] != b'\n' {
                out[i] = b' ';
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                let end = src[i..].find('\n').map_or(b.len(), |e| i + e);
                blank(&mut out, i..end);
                i = end;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut depth = 0;
                let mut j = i;
                while j < b.len() {
                    if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                        depth += 1;
                        j += 2;
                    } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                        depth -= 1;
                        j += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        j += 1;
                    }
                }
                blank(&mut out, i..j.min(b.len()));
                i = j;
            }
            b'r' if !ident_before(b, i) && matches!(b.get(i + 1), Some(b'"' | b'#')) => {
                // Raw string: r"..." or r#"..."#.
                let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                let open = i + 1 + hashes;
                if b.get(open) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let close = format!("\"{}", "#".repeat(hashes));
                let end = src[open + 1..]
                    .find(&close)
                    .map_or(b.len(), |e| open + 1 + e);
                if strings {
                    blank(&mut out, open + 1..end);
                }
                i = end + close.len();
            }
            b'"' => {
                let mut j = i + 1;
                while j < b.len() && b[j] != b'"' {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                if strings {
                    blank(&mut out, i + 1..j.min(b.len()));
                }
                i = j + 1;
            }
            b'\'' => {
                // A char literal ('x', '\n', '\u{1F600}'), not a lifetime.
                let end = if b.get(i + 1) == Some(&b'\\') {
                    src[i + 2..].find('\'').map(|e| i + 2 + e)
                } else {
                    let len = src[i + 1..].chars().next().map_or(1, char::len_utf8);
                    (b.get(i + 1 + len) == Some(&b'\'')).then_some(i + 1 + len)
                };
                match end {
                    Some(end) => {
                        if strings {
                            blank(&mut out, i + 1..end);
                        }
                        i = end + 1;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).expect("masking keeps UTF-8 boundaries")
}

fn ident_before(b: &[u8], i: usize) -> bool {
    i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')
}

/// The byte ranges of `#[cfg(test)]` items in masked code: from the attribute
/// to the brace that closes the item's body. An item without a body (`mod
/// name;`) has no range here; its file is found by `test_only_files`.
fn test_regions(code: &str) -> Vec<Range<usize>> {
    let b = code.as_bytes();
    let mut regions = Vec::new();
    for (start, _) in code.match_indices("#[cfg(test)]") {
        let after = start + "#[cfg(test)]".len();
        let Some(open) = code[after..].find(['{', ';']).map(|o| after + o) else {
            continue;
        };
        if b[open] == b';' {
            continue;
        }
        let mut depth = 0usize;
        let mut end = b.len();
        for (j, &c) in b.iter().enumerate().skip(open) {
            match c {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = j + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        regions.push(start..end);
    }
    regions
}

/// Files that are test modules by their declaration: `#[cfg(test)] mod name;`
/// in a parent, resolved to `name.rs` or `name/mod.rs` beside it.
fn test_only_files(sources: &[(String, String)]) -> HashSet<String> {
    let decl = Regex::new(r"#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;")
        .expect("module declaration pattern");
    let known: HashSet<&str> = sources.iter().map(|(rel, _)| rel.as_str()).collect();
    let mut out = HashSet::new();
    for (rel, src) in sources {
        let path = Path::new(rel);
        let dir = match path.file_stem().and_then(|s| s.to_str()) {
            Some("mod" | "lib" | "main") => path.parent().map(Path::to_path_buf),
            _ => Some(path.with_extension("")),
        }
        .unwrap_or_default();
        for caps in decl.captures_iter(&mask(src, true)) {
            let name = &caps[1];
            for candidate in [
                dir.join(format!("{name}.rs")),
                dir.join(name).join("mod.rs"),
            ] {
                let candidate = candidate.to_string_lossy().replace('\\', "/");
                if known.contains(candidate.as_str()) {
                    out.insert(candidate);
                }
            }
        }
    }
    out
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("under src")
        .to_string_lossy()
        .replace('\\', "/")
}
