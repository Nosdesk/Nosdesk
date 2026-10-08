//! Lint: every elevated database call says why.
//!
//! `background_run`, `with_actor_bypass_context` and `elevate_session_role`
//! run on the BYPASSRLS role, so row security does NOT scope their reads by
//! workspace. That is correct for genuinely cross-workspace work (a queue
//! drain across every tenant, the search reindexer, pre-auth workspace
//! resolution, a global or untenanted table) and for the few single-workspace
//! steps row security can't express (creating the workspace itself, reading a
//! membership the caller doesn't have yet). Anywhere else it is a footgun: an
//! unfiltered read silently returns an arbitrary tenant's rows (the shape of
//! the B1 cross-tenant webhook bug). `run_in_workspace` / `with_actor_context`
//! are the safe default.
//!
//! This lint does NOT try to prove a call is workspace-safe (impossible
//! statically: the same repo fn is called from both contexts, and a legitimate
//! cross-tenant drain looks identical to a forgotten filter). Instead it makes
//! every elevated call a deliberate, reviewed act: the author states, inline,
//! why it needs to bypass row security. A new call with no reason fails CI.
//!
//! ## The markers
//!
//! A comment in the contiguous comment block directly above the call:
//!
//! - `// cross-tenant: <reason>` when the work spans workspaces (or has none);
//! - `// elevated-in-workspace: <reason>` when the work is one workspace's but
//!   needs a row security policy lifted (say which and why).
//!
//! ```ignore
//! // cross-tenant: queue drain claims outbound rows across every tenant.
//! let batch = background_run(&pool, "background:email_drain", |conn| { ... })?;
//! ```
//!
//! `background_run` takes only `cross-tenant:` (it pins no workspace). If the
//! work is single-workspace and needs nothing lifted, don't add a marker:
//! switch to `run_in_workspace` / `with_actor_context` so row security scopes
//! it.
//!
//! `src/sync/session.rs` (which defines and unit-tests the primitives) is
//! exempt. So are test modules for the bypass primitives: fixtures seed and
//! inspect across workspaces on purpose.

use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// File that defines the primitives and their tests.
const EXEMPT_RELPATH: &str = "sync/session.rs";
const CROSS_TENANT: &str = "cross-tenant:";
const IN_WORKSPACE: &str = "elevated-in-workspace:";

#[derive(Debug)]
struct Violation {
    relpath: String,
    line: usize,
    snippet: String,
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// True when the contiguous `//` comment block immediately above `idx` (or
/// above the `let x =` line the call continues) contains one of `markers`.
fn has_marker_above(lines: &[&str], idx: usize, markers: &[&str]) -> bool {
    let mut i = idx;
    // A call on the line after `let x =` belongs to that statement.
    while i > 0 && !is_comment(lines[i - 1]) && lines[i - 1].trim_end().ends_with('=') {
        i -= 1;
    }
    while i > 0 && is_comment(lines[i - 1]) {
        if markers.iter().any(|m| lines[i - 1].contains(m)) {
            return true;
        }
        i -= 1;
    }
    false
}

/// Whether `line` calls `name`, with or without a turbofish
/// (`name(` / `name::<T, E>(`).
fn calls(line: &str, name: &str) -> bool {
    let mut rest = line;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + name.len()..].trim_start();
        let is_ident_start = before.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if is_ident_start && (after.starts_with('(') || after.starts_with("::<")) {
            return true;
        }
        rest = &rest[at + name.len()..];
    }
    false
}

/// Lines (0-based) inside a `#[cfg(test)] mod ... { }` block, by brace depth.
fn test_module_lines(lines: &[&str]) -> Vec<bool> {
    let mut in_test = vec![false; lines.len()];
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "#[cfg(test)]" {
            // The module header follows the attribute (possibly after other
            // attributes).
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim_start().starts_with("#[") {
                j += 1;
            }
            let header = lines.get(j).map(|l| l.trim_start()).unwrap_or("");
            let is_mod = header.starts_with("mod ") || header.starts_with("pub mod ");
            if is_mod && header.ends_with('{') {
                let mut depth: i64 = 0;
                let mut k = j;
                while k < lines.len() {
                    depth += lines[k].matches('{').count() as i64;
                    depth -= lines[k].matches('}').count() as i64;
                    in_test[k] = true;
                    if depth <= 0 && k > j {
                        break;
                    }
                    k += 1;
                }
                i = k + 1;
                continue;
            }
        }
        i += 1;
    }
    in_test
}

#[test]
fn every_elevated_call_says_why() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(src_root.exists(), "src not found at {}", src_root.display());
    let exempt = src_root.join(EXEMPT_RELPATH);

    let mut violations: Vec<Violation> = Vec::new();

    for entry in WalkDir::new(&src_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().map(|x| x == "rs").unwrap_or(false))
    {
        let path: &Path = entry.path();
        if path == exempt {
            continue;
        }
        let relpath = path
            .strip_prefix(&src_root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        // A whole file of tests (`#[cfg(test)] mod tests;`).
        let test_file = relpath.ends_with("/tests.rs") || relpath == "tests.rs";
        let content = fs::read_to_string(path).unwrap_or_default();
        let lines: Vec<&str> = content.lines().collect();
        let in_test = test_module_lines(&lines);

        for (idx, raw) in lines.iter().enumerate() {
            // Skips prose that mentions a primitive.
            if is_comment(raw) {
                continue;
            }
            let markers: &[&str] = if calls(raw, "background_run") {
                &[CROSS_TENANT]
            } else if (calls(raw, "with_actor_bypass_context")
                || calls(raw, "elevate_session_role"))
                && !(test_file || in_test[idx])
            {
                &[CROSS_TENANT, IN_WORKSPACE]
            } else {
                continue;
            };
            if has_marker_above(&lines, idx, markers) {
                continue;
            }
            violations.push(Violation {
                relpath: relpath.clone(),
                line: idx + 1,
                snippet: raw.trim().to_string(),
            });
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "\nElevated database calls missing a reason. Above each, add\n\
             `// cross-tenant: <reason>` (the work spans workspaces) or, for\n\
             with_actor_bypass_context / elevate_session_role only,\n\
             `// elevated-in-workspace: <reason>` (one workspace's work that\n\
             needs a row security policy lifted). If neither applies, use\n\
             run_in_workspace / with_actor_context instead.\n\n",
        );
        for v in &violations {
            msg.push_str(&format!("  src/{}:{}  {}\n", v.relpath, v.line, v.snippet));
        }
        panic!("{msg}");
    }
}

#[test]
fn the_lint_sees_turbofish_calls_and_skips_definitions_and_lookalikes() {
    assert!(calls(
        "with_actor_bypass_context(&mut conn, &a, f)",
        "with_actor_bypass_context"
    ));
    assert!(calls(
        "x = session::with_actor_bypass_context::<_, Error>(conn, a, f)",
        "with_actor_bypass_context"
    ));
    assert!(calls(
        "elevate_session_role(&mut conn, &actor)",
        "elevate_session_role"
    ));
    assert!(!calls(
        "my_with_actor_bypass_context(conn)",
        "with_actor_bypass_context"
    ));
    assert!(!calls(
        "use crate::sync::session::with_actor_bypass_context;",
        "with_actor_bypass_context"
    ));

    let src = [
        "fn live() { with_actor_bypass_context(c, a, f) }",
        "#[cfg(test)]",
        "mod tests {",
        "    fn t() { with_actor_bypass_context(c, a, f) }",
        "}",
        "fn after() {}",
    ];
    let continued = [
        "// elevated-in-workspace: reads a membership the caller doesn't have yet.",
        "let result =",
        "    with_actor_bypass_context(c, a, f);",
    ];
    assert!(has_marker_above(&continued, 2, &[IN_WORKSPACE]));
    assert!(!has_marker_above(&continued, 2, &[CROSS_TENANT]));

    assert_eq!(
        test_module_lines(&src),
        vec![false, false, true, true, true, false]
    );
}
