//! Lint: every `background_run(...)` call carries a `// cross-tenant:` marker
//! justifying why it uses the BYPASSRLS primitive instead of the workspace-
//! pinned `run_in_workspace`.
//!
//! `background_run` runs its closure on a BYPASSRLS connection, so RLS does NOT
//! scope its reads by workspace. That is correct ONLY for genuinely
//! cross-workspace work (a queue drain across every tenant, the search
//! reindexer, pre-auth workspace resolution, a global/untenanted table). For
//! single-workspace work it is a footgun: an unfiltered read silently returns
//! an arbitrary tenant's rows (this is the shape of the B1 cross-tenant webhook
//! bug). `run_in_workspace(workspace_id, ...)` is the safe default there.
//!
//! This lint does NOT try to prove a call is workspace-safe (impossible
//! statically: the same repo fn is called from both contexts, and a legitimate
//! cross-tenant drain looks identical to a forgotten filter). Instead it forces
//! every BYPASSRLS use to be a deliberate, reviewed act: the author must state,
//! inline, why crossing tenants is correct here. A new `background_run` with no
//! justification fails CI, which is where the next B1-class omission surfaces.
//!
//! ## The marker
//!
//! A `// cross-tenant: <reason>` comment in the contiguous comment block
//! directly above the call:
//!
//! ```ignore
//! // cross-tenant: queue drain claims outbound rows across every tenant.
//! let batch = background_run(&pool, "background:email_drain", |conn| { ... })?;
//! ```
//!
//! If the work is actually single-workspace, do not add a marker: switch the
//! call to `run_in_workspace(workspace_id, ...)` so RLS scopes it.
//!
//! `src/sync/session.rs` (which defines and unit-tests both primitives) is
//! exempt.

use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// File that defines `background_run` / `run_in_workspace` and their tests.
const EXEMPT_RELPATH: &str = "sync/session.rs";
const MARKER: &str = "cross-tenant:";

#[derive(Debug)]
struct Violation {
    relpath: String,
    line: usize,
    snippet: String,
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// True when the contiguous `//` comment block immediately above `idx`
/// contains the marker.
fn has_marker_above(lines: &[&str], idx: usize) -> bool {
    let mut i = idx;
    while i > 0 && is_comment(lines[i - 1]) {
        if lines[i - 1].contains(MARKER) {
            return true;
        }
        i -= 1;
    }
    false
}

#[test]
fn every_background_run_declares_cross_tenant_justification() {
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
        let content = fs::read_to_string(path).unwrap_or_default();
        let lines: Vec<&str> = content.lines().collect();

        for (idx, raw) in lines.iter().enumerate() {
            // A call site: contains `background_run(` and is not itself a
            // comment (skips prose that mentions the primitive).
            if !raw.contains("background_run(") || is_comment(raw) {
                continue;
            }
            if has_marker_above(&lines, idx) {
                continue;
            }
            violations.push(Violation {
                relpath: path
                    .strip_prefix(&src_root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned(),
                line: idx + 1,
                snippet: raw.trim().to_string(),
            });
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "\nbackground_run() calls missing a `// cross-tenant: <reason>` marker.\n\
             Either add the marker (justify why crossing tenants is correct) or\n\
             switch to run_in_workspace(workspace_id, ...) if the work is\n\
             single-workspace.\n\n",
        );
        for v in &violations {
            msg.push_str(&format!("  src/{}:{}  {}\n", v.relpath, v.line, v.snippet));
        }
        panic!("{msg}");
    }
}
