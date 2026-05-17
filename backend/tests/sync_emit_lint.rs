//! Lint: every repository write either calls `sync::emit::record` or
//! carries an inline marker comment declaring why it doesn't.
//!
//! The marker lives directly above the `pub fn`, on the line preceding
//! any doc comments or attributes:
//!
//! ```ignore
//! // sync-audit-only: covered by security_events
//! /// Delete the session row.
//! pub fn revoke_session(...) -> QueryResult<usize> { ... }
//! ```
//!
//! Two marker forms:
//!
//! - `// sync-audit-only: <reason>` — the write intentionally does not
//!   emit a sync_action. Operational tables (queues, retention logs),
//!   security-only writes covered by `security_events`, or aggregates
//!   no sync client subscribes to.
//! - `// sync-pending-wire: <todo>` — the write *should* emit, but the
//!   sync aggregate variant + registry manifest haven't landed yet.
//!   Removing the marker is part of the commit that wires the emit.
//!
//! Why per-function markers instead of a central allowlist: a central
//! list silently drifts when new repo writes appear (the lint never
//! ran until the friend's CI workflow landed; the list had ~30
//! missing entries). Inline markers force the audit decision to be
//! visible in the PR that adds the fn, and the rationale lives next
//! to the code instead of in a file no one reads.

use regex::Regex;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

#[derive(Debug)]
struct ReportEntry {
    relpath: String,
    fn_name: String,
}

#[test]
fn every_repository_write_calls_sync_emit_or_carries_marker() {
    let repo_root: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/repository");
    assert!(
        repo_root.exists(),
        "repository directory not found at {}",
        repo_root.display()
    );

    let mut violations: Vec<ReportEntry> = Vec::new();

    let fn_re = Regex::new(r"(?m)^(?P<indent>[ \t]*)pub(?:\s*\([^)]*\))?\s+fn\s+(?P<name>\w+)\s*[<(]").unwrap();
    let write_re = Regex::new(
        r"diesel::insert_into\s*\(|diesel::update\s*\(|diesel::delete\s*\(|diesel::sql_query\s*\(",
    )
    .unwrap();

    for entry in WalkDir::new(&repo_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
    {
        let path = entry.path();
        let relpath = format!(
            "repository/{}",
            path.strip_prefix(&repo_root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        );
        // The mod root has no `pub fn` of its own.
        if relpath == "repository/mod.rs" {
            continue;
        }

        let src = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let src = strip_test_modules(&src);
        let src = strip_block_comments(&src);

        for func in iter_pub_fns(&src, &fn_re) {
            if !write_re.is_match(&func.body) {
                continue;
            }
            // Treat both qualified and unqualified calls as wired.
            let wired = func.body.contains("emit::record")
                || func.body.contains("sync::emit::record")
                || func.body.contains("sync_emit::record");
            if wired {
                continue;
            }
            if func.has_marker {
                continue;
            }
            violations.push(ReportEntry {
                relpath: relpath.clone(),
                fn_name: func.name.clone(),
            });
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "\nUnwired repository writes found. Either call sync::emit::record\n\
             in the same transaction, or declare why the write is audit-only\n\
             by adding an inline marker directly above the pub fn:\n\n  \
             // sync-audit-only: <reason>\n  \
             // sync-pending-wire: <todo>\n\n\
             Functions missing both an emit call and a marker:\n\n",
        );
        for v in &violations {
            msg.push_str(&format!("  {}::{}\n", v.relpath, v.fn_name));
        }
        panic!("{msg}");
    }
}

struct PubFn {
    name: String,
    body: String,
    has_marker: bool,
}

/// Find every `pub fn` in the source, capture its body, and note
/// whether the line above (skipping doc comments and attributes)
/// carries a `// sync-audit-only:` or `// sync-pending-wire:` marker.
fn iter_pub_fns(src: &str, fn_re: &Regex) -> Vec<PubFn> {
    let mut out = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    // Pre-compute byte offset of each line start so we can map a
    // regex match position back to a 0-indexed line number.
    let mut line_starts = Vec::with_capacity(lines.len() + 1);
    let mut off = 0usize;
    for line in &lines {
        line_starts.push(off);
        off += line.len() + 1; // +1 for the '\n'
    }
    line_starts.push(off);

    for caps in fn_re.captures_iter(src) {
        let name = caps.name("name").unwrap().as_str().to_string();
        let match_start = caps.get(0).unwrap().start();
        let header_end = caps.get(0).unwrap().end();

        // Body extraction (matching the original brace counter).
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

        // Find the line number where the `pub fn` lives, then walk
        // backwards skipping doc comments / attribute lines / blank
        // lines, and check whether the resulting line is a marker.
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
        let has_marker = cursor > 0 && {
            let prev = lines[cursor - 1].trim_start();
            prev.starts_with("// sync-audit-only:") || prev.starts_with("// sync-pending-wire:")
        };

        out.push(PubFn {
            name,
            body,
            has_marker,
        });
    }
    out
}

/// Drop `#[cfg(test)] mod tests { ... }` blocks so test fixtures
/// that intentionally bypass emit don't trip the lint.
///
/// The attribute is matched only at the start of a line (after
/// optional whitespace) so a `#[cfg(test)]` mention inside a doc
/// comment doesn't accidentally swallow real code that happens to
/// follow the comment. The attribute must also be immediately
/// followed by a `mod ` declaration on the same or next non-blank
/// line — `#[cfg(test)] fn ...` annotations on individual functions
/// are intentionally left intact (and any diesel write inside such a
/// fn would be a real lint violation).
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
                    let bytes = lines[k].as_bytes();
                    for &b in bytes {
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

/// Strip /* ... */ block comments so a sample `diesel::update(` in a
/// docstring example doesn't trip the write detector. Line-style doc
/// comments (`///`) are left alone — they're needed for marker lookup
/// when walking backwards from a `pub fn`.
fn strip_block_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let bytes = src.as_bytes();
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            if let Some(end) = src[i + 2..].find("*/") {
                // Replace with same number of newlines so line numbers
                // outside the comment stay stable for marker lookup.
                let span = &src[i..i + 2 + end + 2];
                for c in span.chars() {
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
        out.push(src[i..].chars().next().unwrap());
        i += src[i..].chars().next().unwrap().len_utf8();
    }
    out
}
