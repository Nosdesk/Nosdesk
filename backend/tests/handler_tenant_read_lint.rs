//! Lint (defense-in-depth): a handler should not make an authorization-relevant
//! tenant read on a raw (un-pinned) database connection.
//!
//! This is a DEV-TIME CORRECTNESS guardrail, NOT the tenant-isolation control.
//! Isolation is enforced structurally below the app and does not depend on this
//! lint:
//!   - FORCE row-level security scopes every tenant query by `app.workspace_id`,
//!     so a read on a raw `pool.get()` / `db_conn()` connection (the pool clears
//!     the GUC on checkout) returns ZERO rows under the NOBYPASSRLS `nosdesk_app`
//!     role, and
//!   - the P0.2 startup guard (`main.rs`) refuses to boot a hosted PRODUCTION
//!     deployment connected as a role that bypasses RLS, so RLS is guaranteed
//!     active there. An unpinned tenant read therefore cannot leak cross-tenant
//!     in production — for every reader, not just the ones named below.
//!
//! What this lint catches is the CORRECTNESS bug that slips under those
//! guarantees: a handler that does an ad-hoc tenant read on a raw connection to
//! make an authorization decision fails closed (the read finds nothing, so it
//! 404s even for the caller's own resource) and skips the app-layer membership /
//! visibility gate entirely. That is how `upload_ticket_note_image` regressed:
//! it called `get_ticket_by_id` on a raw connection instead of going through
//! `TenantConn` + `authorize_ticket_access`. `audit_context_lint` is the
//! write-side equivalent (emitting repo fns); this is its read-side companion.
//!
//! ## What is matched
//!
//! A handler is flagged when it holds a raw connection (`pool.get()` /
//! `db_conn(`), does NOT pin it (`with_actor_context` / `with_actor_bypass_context`
//! / `pin_workspace` / `pin_request_workspace` / `run_in_workspace` /
//! `background_run`), is NOT exempt, AND calls one of:
//!   - the access-check family, matched by naming convention: `authorize_*`,
//!     `can_view_*`, `can_access_*`, `can_user_access_*` (these MUST run pinned),
//!   - a known handler-level tenant existence reader (see `TENANT_READERS`).
//!
//! ## Escape hatch
//!
//! Put `// tenant-read-exempt: <reason>` directly above the `pub [async] fn`
//! when the read is provably safe (e.g. the conn is pinned in a helper the
//! scanner can't see, or the read is a platform/global table, not tenant data).

use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Handler-level tenant readers that produce an authorization-relevant result.
/// Extend this when a new handler does an ad-hoc tenant existence/lookup check
/// on a connection it holds (prefer routing through `authorize_*` instead).
const TENANT_READERS: &[&str] = &["get_ticket_by_id", "get_complete_ticket"];

/// Access-check fns are matched by these name prefixes, so the list never drifts
/// as new ones are added — any `authorize_*` / `can_view_*` etc. must run pinned.
const ACCESS_PREFIXES: &[&str] = &["authorize_", "can_view_", "can_access_", "can_user_access_"];

#[derive(Debug)]
struct Violation {
    relpath: String,
    handler: String,
    called: String,
}

#[test]
fn handler_authz_reads_run_on_a_pinned_connection() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let handler_root = manifest.join("src/handlers");
    assert!(handler_root.exists());

    let fn_re = Regex::new(
        r"(?m)^(?P<indent>[ \t]*)pub(?:\s*\([^)]*\))?\s+(?:async\s+)?fn\s+(?P<name>\w+)\s*[<(]",
    )
    .unwrap();
    let raw_conn_re = Regex::new(r"\bdb_conn\s*\(|\bpool\s*\.\s*get\s*\(").unwrap();
    let pin_re = Regex::new(
        r"with_actor_context|with_actor_bypass_context|pin_workspace|pin_request_workspace|run_in_workspace|background_run",
    )
    .unwrap();
    // Calls to a matched authorization read: any access-prefix fn, or one of the
    // explicit tenant readers. `\bNAME\s*(`.
    let mut patterns: Vec<String> = ACCESS_PREFIXES
        .iter()
        .map(|p| format!(r"\b{}\w+\s*\(", regex::escape(p)))
        .collect();
    patterns.extend(
        TENANT_READERS
            .iter()
            .map(|n| format!(r"\b{}\s*\(", regex::escape(n))),
    );
    let call_re = Regex::new(&patterns.join("|")).unwrap();
    // Extract which name actually matched, for the error message.
    let name_re = Regex::new(r"\b(\w+)\s*\(").unwrap();

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
            if func.exempt || !raw_conn_re.is_match(&func.body) || pin_re.is_match(&func.body) {
                continue;
            }
            if let Some(m) = call_re.find(&func.body) {
                let called = name_re
                    .captures(m.as_str())
                    .and_then(|c| c.get(1))
                    .map(|g| g.as_str().to_string())
                    .unwrap_or_else(|| m.as_str().to_string());
                violations.push(Violation {
                    relpath: relpath.clone(),
                    handler: func.name.clone(),
                    called,
                });
            }
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "\nHandlers make an authorization-relevant tenant read on a RAW (un-pinned)\n\
             connection. Under the NOBYPASSRLS app role this fails closed (RLS-zero),\n\
             so it 404s in production and bypasses the app-layer membership/visibility\n\
             gate. (Isolation itself is RLS + the P0.2 boot guard; this is a\n\
             correctness/defense-in-depth nudge.) Fix by either:\n\
             - taking the `TenantConn` extractor and routing the read through it\n\
               (preferred: `authorize_ticket_access(&mut tc, &auth, id)`), or\n\
             - pinning the connection (`pin_workspace` / `with_actor_context`),\n\
             or, if provably safe, add directly above the fn:\n  \
             // tenant-read-exempt: <reason>\n\n\
             Offending handlers:\n\n",
        );
        for v in &violations {
            msg.push_str(&format!(
                "  {}::{}  (calls `{}` on a raw conn)\n",
                v.relpath, v.handler, v.called
            ));
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
                .starts_with("// tenant-read-exempt:");

        out.push(PubFn { name, body, exempt });
    }
    out
}

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

#[allow(dead_code)]
fn _unused(_: &Path) {}
