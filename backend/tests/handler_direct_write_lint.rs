//! Lint: a handler must not write the database directly. Every data
//! mutation belongs in a `repository::*` function, where the
//! `sync_emit_lint` emit/marker contract applies.
//!
//! ## Why this exists
//!
//! `sync_emit_lint` only walks `src/repository`, and `audit_context_lint`
//! only fires when a handler calls a *named emitting repo fn* on a raw
//! connection. A handler that writes with a bare `diesel::insert_into(...)`
//! (calling no repo fn at all) slips past BOTH: the write reaches no
//! `sync::emit::record`, subscribes to no aggregate, and nothing notices.
//! That is exactly how a future handler-direct write to a sync-subscribed
//! table (tickets, comments, attachments, ...) would silently stop
//! emitting. This lint closes that gap by forbidding the pattern outright:
//! writes live in repositories, and the repository lints take it from
//! there.
//!
//! ## What is matched
//!
//! Inside any handler `pub [async] fn`, a Diesel DSL write
//! (`insert_into` / `update` / `delete`) or a raw `sql_query` whose
//! statement begins with `INSERT` / `UPDATE` / `DELETE`. Read-only
//! `sql_query` SELECTs and GUC statements (`SET LOCAL`, `set_config`)
//! are not writes and are ignored.
//!
//! ## Escape hatch
//!
//! Put `// handler-write-exempt: <reason>` directly above the
//! `pub [async] fn` when a direct write genuinely has to live in the
//! handler (there is no such case today; the marker exists so a
//! deliberate exception is visible in review rather than silent).

use regex::Regex;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

#[derive(Debug)]
struct Violation {
    relpath: String,
    handler: String,
}

#[test]
fn handlers_contain_no_direct_database_writes() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let handler_root = manifest.join("src/handlers");
    assert!(handler_root.exists());

    let fn_re = Regex::new(
        r"(?m)^(?P<indent>[ \t]*)pub(?:\s*\([^)]*\))?\s+(?:async\s+)?fn\s+(?P<name>\w+)\s*[<(]",
    )
    .unwrap();
    // Diesel DSL writes, plus a raw sql_query whose first keyword is a
    // write verb. SELECT / SET LOCAL / `SELECT set_config(...)` do not
    // match, so GUC pins and ad-hoc reads are left alone.
    let write_re = Regex::new(
        r#"diesel::insert_into\s*\(|diesel::update\s*\(|diesel::delete\s*\(|diesel::sql_query\s*\(\s*(?:r#*)?"\s*(?i:INSERT|UPDATE|DELETE)\b"#,
    )
    .unwrap();

    let mut violations: Vec<Violation> = Vec::new();

    for entry in WalkDir::new(&handler_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
    {
        let path = entry.path();
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let src = strip_block_comments(&strip_test_modules(&raw));
        let relpath = format!(
            "handlers/{}",
            path.strip_prefix(&handler_root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        );

        for func in iter_pub_fns(&src, &fn_re) {
            if func.exempt {
                continue;
            }
            if write_re.is_match(&func.body) {
                violations.push(Violation {
                    relpath: relpath.clone(),
                    handler: func.name.clone(),
                });
            }
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "\nHandlers write the database directly. A bare diesel write in a\n\
             handler bypasses the repository layer, so it emits no sync_action\n\
             and no lint sees it. Move the write into a `repository::*` fn\n\
             (where sync_emit_lint enforces the emit/marker contract), or, if a\n\
             direct write is genuinely unavoidable, add directly above the fn:\n  \
             // handler-write-exempt: <reason>\n\n\
             Offending handlers:\n\n",
        );
        for v in &violations {
            msg.push_str(&format!("  {}::{}\n", v.relpath, v.handler));
        }
        panic!("{msg}");
    }
}

// ---------------------------------------------------------------------------
// Scanning helpers (mirrors audit_context_lint.rs / sync_emit_lint.rs;
// integration-test crates can't share private items, so the small scanners
// are duplicated.)
// ---------------------------------------------------------------------------

struct PubFn {
    name: String,
    body: String,
    exempt: bool,
}

fn iter_pub_fns(src: &str, fn_re: &Regex) -> Vec<PubFn> {
    let mut out = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut line_starts = Vec::with_capacity(lines.len() + 1);
    let mut off = 0usize;
    for line in &lines {
        line_starts.push(off);
        off += line.len() + 1;
    }
    line_starts.push(off);

    for caps in fn_re.captures_iter(src) {
        let name = caps.name("name").unwrap().as_str().to_string();
        let match_start = caps.get(0).unwrap().start();
        let header_end = caps.get(0).unwrap().end();

        let Some(open_brace) = src[header_end..].find('{') else {
            continue;
        };
        let body_start = header_end + open_brace + 1;
        let mut depth = 1usize;
        let mut i = body_start;
        let bytes = src.as_bytes();
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let body = src[body_start..i.saturating_sub(1)].to_string();

        let pub_line = line_starts.partition_point(|&start| start <= match_start) - 1;
        let mut cursor = pub_line;
        while cursor > 0 {
            let prev = lines[cursor - 1].trim_start();
            if prev.starts_with("///")
                || prev.starts_with("//!")
                || prev.starts_with("#[")
                || prev.is_empty()
            {
                cursor -= 1;
                continue;
            }
            break;
        }
        let exempt = cursor > 0
            && lines[cursor - 1]
                .trim_start()
                .starts_with("// handler-write-exempt:");

        out.push(PubFn { name, body, exempt });
    }
    out
}

/// Drop `#[cfg(test)] mod tests { ... }` blocks so test fixtures don't
/// trip the scan.
fn strip_test_modules(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut keep: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0usize;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
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
                    for &b in lines[k].as_bytes() {
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
        keep.push(lines[i]);
        i += 1;
    }
    keep.join("\n")
}

/// Strip `/* ... */` block comments, preserving newlines so line
/// numbers stay stable for the marker lookup.
fn strip_block_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let bytes = src.as_bytes();
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            if let Some(end) = src[i + 2..].find("*/") {
                for c in src[i..i + 2 + end + 2].chars() {
                    if c == '\n' {
                        out.push('\n');
                    }
                }
                i = i + 2 + end + 2;
                continue;
            }
            out.push_str(&src[i..]);
            break;
        }
        let ch = src[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}
