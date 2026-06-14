//! Lint: a handler that acquires a raw DB connection (`db_conn`) and
//! calls an emitting repository fn must run that work inside actor
//! context (`with_actor_context` / `with_actor_bypass_context`), or go
//! through a `TenantConn` / `PlatformConn` extractor (which never hand
//! the handler a raw connection). Otherwise the write to an audited
//! table reaches the `audit_log` trigger with no `app.workspace_id`
//! set and fails with NDX01 at runtime — a 500 that only surfaces the
//! first time the endpoint is exercised.
//!
//! This is the handler-layer companion to `sync_emit_lint`, which
//! enforces the repository-layer emit/marker contract. In hindsight it
//! catches both the `update_user_by_uuid` and `create_user`
//! regressions (raw `db_conn` + an emitting repo call + no wrapping).
//!
//! ## Precision
//!
//! Emitting repo fns are matched in handlers by bare name. Generic
//! names (`update`, `create`, `delete`) recur across repository
//! modules, so matching them by bare name would collide and produce
//! false positives. To stay sound, only emitting fns whose name is
//! UNIQUE across the entire repository layer are matched. That makes
//! the lint zero-false-positive at the cost of not catching
//! generic-named emitting fns — those are covered by the extractor
//! boundary (a handler can't call them on a raw conn it doesn't have).
//! Defense-in-depth, not the sole guard.
//!
//! ## Escape hatch
//!
//! Put `// audit-context-exempt: <reason>` directly above the
//! `pub [async] fn` when the raw-conn call is provably safe — e.g. the
//! emitting call is handed a connection from a separately-wrapped
//! scope, or the flow only writes de-audited platform tables.

use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug)]
struct Violation {
    relpath: String,
    handler: String,
    emitting_fn: String,
}

#[test]
fn handler_raw_conn_emitting_calls_are_actor_wrapped() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest.join("src/repository");
    let handler_root = manifest.join("src/handlers");
    assert!(repo_root.exists() && handler_root.exists());

    let fn_re = Regex::new(
        r"(?m)^(?P<indent>[ \t]*)pub(?:\s*\([^)]*\))?\s+(?:async\s+)?fn\s+(?P<name>\w+)\s*[<(]",
    )
    .unwrap();
    let emit_re = Regex::new(r"emit::record|sync::emit::record|sync_emit::record").unwrap();
    // A handler "holds a raw connection" when it pulls one straight from
    // the pool, by either of the two idioms in the codebase:
    // `helpers::db_conn(&pool)` or `pool.get()`. The latter is what let
    // upload_files slip an un-pinned attachment insert past this lint.
    let raw_conn_re = Regex::new(r"\bdb_conn\s*\(|\bpool\s*\.\s*get\s*\(").unwrap();

    // ---- Pass 1: build the set of UNIQUELY-named emitting repo fns. ----
    //
    // `defs` counts every pub fn definition per bare name across the
    // repository layer; `emitters` counts those whose body emits. A
    // name is safe to match in handlers only when it is defined exactly
    // once and that one definition emits.
    let mut defs: HashMap<String, usize> = HashMap::new();
    let mut emitters: HashMap<String, usize> = HashMap::new();

    for src in rust_sources(&repo_root) {
        let src = strip_block_comments(&strip_test_modules(&src));
        for func in iter_pub_fns(&src, &fn_re) {
            *defs.entry(func.name.clone()).or_insert(0) += 1;
            // A self-wrapping emitter (one that takes an actor and opens
            // its own `with_actor_context` transaction, e.g.
            // `rules::apply_manual`) is safe to call on a raw conn — it
            // establishes context itself. Exclude it from the matchable
            // set so its handler callers aren't flagged.
            let self_wraps = func.body.contains("with_actor_context")
                || func.body.contains("with_actor_bypass_context");
            if emit_re.is_match(&func.body) && !self_wraps {
                *emitters.entry(func.name.clone()).or_insert(0) += 1;
            }
        }
    }

    let distinctive_emitters: Vec<String> = emitters
        .keys()
        .filter(|name| defs.get(*name).copied() == Some(1))
        .cloned()
        .collect();
    assert!(
        !distinctive_emitters.is_empty(),
        "expected at least one uniquely-named emitting repo fn; the scan likely broke"
    );

    // Pre-compile a call matcher per distinctive emitter: `\bNAME\s*(`.
    let call_res: Vec<(String, Regex)> = distinctive_emitters
        .iter()
        .map(|name| {
            (
                name.clone(),
                Regex::new(&format!(r"\b{}\s*\(", regex::escape(name))).unwrap(),
            )
        })
        .collect();

    // ---- Pass 2: scan handlers for raw-conn + emitting-call + no wrap. ----
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
            // Only handlers that hold a raw connection are at risk; the
            // extractors never expose one.
            if !raw_conn_re.is_match(&func.body) {
                continue;
            }
            // Wrapped (manual context) is fine.
            if func.body.contains("with_actor_context")
                || func.body.contains("with_actor_bypass_context")
            {
                continue;
            }
            for (name, call_re) in &call_res {
                if call_re.is_match(&func.body) {
                    violations.push(Violation {
                        relpath: relpath.clone(),
                        handler: func.name.clone(),
                        emitting_fn: name.clone(),
                    });
                }
            }
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "\nHandlers call an emitting repository fn on a raw `db_conn` without\n\
             actor context. The audited write will fail with NDX01 (missing\n\
             app.workspace_id) at runtime. Fix by either:\n\
             - migrating the handler to the `TenantConn` extractor, or\n\
             - wrapping the call in `with_actor_context(&mut conn, &actor, |c| ...)`,\n\
             or, if the call is provably safe, add directly above the fn:\n  \
             // audit-context-exempt: <reason>\n\n\
             Offending handlers:\n\n",
        );
        for v in &violations {
            msg.push_str(&format!(
                "  {}::{}  (calls emitting `{}`)\n",
                v.relpath, v.handler, v.emitting_fn
            ));
        }
        panic!("{msg}");
    }
}

// ---------------------------------------------------------------------------
// Scanning helpers (mirrors sync_emit_lint.rs; integration-test crates
// can't share private items, so the small scanners are duplicated. If a
// third lint appears, lift these into tests/common.)
// ---------------------------------------------------------------------------

struct PubFn {
    name: String,
    body: String,
    /// True when `// audit-context-exempt:` sits directly above the fn
    /// (skipping doc comments / attributes / blank lines).
    exempt: bool,
}

fn rust_sources(root: &Path) -> Vec<String> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .collect()
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
        let exempt = cursor > 0 && {
            lines[cursor - 1]
                .trim_start()
                .starts_with("// audit-context-exempt:")
        };

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
