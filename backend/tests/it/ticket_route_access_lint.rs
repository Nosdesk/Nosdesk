//! Lint: every route keyed on a ticket checks that the caller may see it.
//!
//! RLS scopes a request to its workspace, not to the tickets a requester may
//! read, so a handler under `/tickets/{id}` or `/tickets/{ticket_id}` has to
//! decide ticket visibility itself. A handler that takes `web::Path<i32>` and
//! `_auth` compiles, routes and returns data, so nothing else catches the
//! missing check.
//!
//! ## Rule
//!
//! Every `.route("…/tickets/{id}…")` or `.route("…/tickets/{ticket_id}…")`
//! registered outside test code (agent API, portal, collaboration) must map to
//! a handler whose body does one of:
//!
//! - takes the `TicketAccess` extractor;
//! - calls `can_view_ticket` directly, or one of `VISIBILITY_HELPERS`, each of
//!   which is checked here to call `can_view_ticket` itself;
//! - applies a staff or admin gate (`STAFF_GATES`). Staff see every ticket in
//!   the workspace, so a staff gate implies visibility.
//!
//! Anything else carries a one-line marker directly above the handler:
//!
//! ```ignore
//! // ticket-access: <why this handler gates differently>
//! pub async fn handler(...)
//! ```
//!
//! Markers are counted against `MAX_MARKERS` so the list stays short.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

const STAFF_GATES: &[&str] = &[
    "can_handle_tickets()",
    "is_workspace_admin()",
    "require_workspace_role(",
];

/// Wrappers that gate a ticket id on `can_view_ticket`, and the file each
/// lives in. The lint confirms the wrapper still calls it.
const VISIBILITY_HELPERS: &[(&str, &str)] = &[
    ("gate_ticket", "src/handlers/collaboration.rs"),
    ("authorize_ticket_access", "src/handlers/files.rs"),
    ("my_ticket", "src/handlers/portal.rs"),
];

const MARKER: &str = "// ticket-access:";
const MAX_MARKERS: usize = 6;

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// A file's source with its top-level test modules cut out, so test-only
/// route registrations and fixtures don't count.
fn non_test_source(path: &Path) -> String {
    let mut src = fs::read_to_string(path).expect("read source");
    let test_mod = Regex::new(r"(?m)^#\[cfg\(test\)\]\s*\n(?:#\[[^\n]*\]\s*\n)*mod ")
        .expect("test module regex");
    while let Some(m) = test_mod.find(&src) {
        let end = src[m.start()..]
            .find("\n}\n")
            .map_or(src.len(), |i| m.start() + i + 3);
        src.replace_range(m.start()..end, "");
    }
    src
}

/// The text of `fn name` in `src`: the comment lines directly above it
/// through the closing brace at column zero.
fn fn_text<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let re = Regex::new(&format!(
        r"(?m)^(?:pub(?:\([^)]*\))? )?(?:async )?fn {name}\b"
    ))
    .ok()?;
    let m = re.find(src)?;
    let end = src[m.start()..]
        .find("\n}\n")
        .map_or(src.len(), |i| m.start() + i);
    // Walk back over contiguous comment and attribute lines.
    let mut start = m.start();
    for line in src[..m.start()].lines().rev() {
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with("#[") {
            start -= line.len() + 1;
        } else {
            break;
        }
    }
    Some(&src[start..end])
}

#[test]
fn ticket_keyed_routes_check_ticket_access() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&manifest.join("src"), &mut files);
    let sources: HashMap<PathBuf, String> = files
        .iter()
        .map(|p| (p.clone(), non_test_source(p)))
        .collect();

    for (helper, file) in VISIBILITY_HELPERS {
        let src = &sources[&manifest.join(file)];
        let body = fn_text(src, helper).unwrap_or_else(|| panic!("{helper} not found in {file}"));
        assert!(
            body.contains("can_view_ticket") || body.contains("can_access_document"),
            "{helper} in {file} no longer checks ticket visibility; drop it from \
             VISIBILITY_HELPERS or restore the check",
        );
    }

    let route_re = Regex::new(
        r#"\.route\(\s*"([^"]*/tickets/\{(?:id|ticket_id)\}[^"]*)"\s*,\s*web::(\w+)\(\)\s*\.to\(\s*([\w:]+)\s*\)"#,
    )
    .expect("route regex");

    let mut failures: Vec<String> = Vec::new();
    let mut markers: Vec<String> = Vec::new();
    let mut routes = 0usize;

    let mut paths: Vec<&PathBuf> = sources.keys().collect();
    paths.sort();
    for reg_path in paths {
        let reg_src = &sources[reg_path];
        for caps in route_re.captures_iter(reg_src) {
            routes += 1;
            let route = &caps[1];
            let method = caps[2].to_uppercase();
            let target = &caps[3];
            let label = format!("{method} {route} -> {target}");

            let segs: Vec<&str> = target.split("::").collect();
            let name = *segs.last().expect("handler name");
            // `crate::handlers::module::name`: look in that module's file.
            // A bare name: the registering file first. Otherwise any handler
            // file, and an ambiguous match is an error rather than a guess.
            let module_file = (segs.len() >= 2)
                .then(|| segs[segs.len() - 2])
                .filter(|m| *m != "handlers" && *m != "crate")
                .map(|m| manifest.join(format!("src/handlers/{m}.rs")));
            let hit: Option<(PathBuf, &str)> = if let Some(f) = module_file {
                sources
                    .get(&f)
                    .and_then(|s| fn_text(s, name))
                    .map(|t| (f.clone(), t))
            } else if let Some(t) = (segs.len() == 1).then(|| fn_text(reg_src, name)).flatten() {
                Some((reg_path.clone(), t))
            } else {
                let handlers_dir = manifest.join("src/handlers");
                let found: Vec<(PathBuf, &str)> = sources
                    .iter()
                    .filter(|(p, _)| p.starts_with(&handlers_dir))
                    .filter_map(|(p, s)| fn_text(s, name).map(|t| (p.clone(), t)))
                    .collect();
                if found.len() > 1 {
                    failures.push(format!("{label}: handler name is ambiguous"));
                    continue;
                }
                found.into_iter().next()
            };
            let Some((file, body)) = hit else {
                failures.push(format!("{label}: handler not found"));
                continue;
            };

            if body.contains("TicketAccess")
                || body.contains("can_view_ticket(")
                || VISIBILITY_HELPERS
                    .iter()
                    .any(|(h, _)| body.contains(&format!("{h}(")))
                || STAFF_GATES.iter().any(|g| body.contains(g))
            {
                continue;
            }
            match body.lines().find(|l| l.trim_start().starts_with(MARKER)) {
                Some(line) => {
                    let reason = line.trim_start()[MARKER.len()..].trim();
                    assert!(
                        !reason.is_empty(),
                        "{label}: a `{MARKER}` marker needs a reason",
                    );
                    let rel = file.strip_prefix(&manifest).unwrap_or(&file).display();
                    let entry = format!("{rel}::{name}");
                    if !markers.contains(&entry) {
                        markers.push(entry);
                    }
                }
                None => failures.push(label),
            }
        }
    }

    assert!(
        routes > 40,
        "matched only {routes} ticket routes; the route regex has likely drifted"
    );
    assert!(
        failures.is_empty(),
        "These ticket-keyed routes reach a handler that never checks whether the \
         caller may see the ticket:\n  {}\n\n\
         Take `TicketAccess` instead of `web::Path<i32>`, call `can_view_ticket`, \
         gate on a staff role, or put `{MARKER} <reason>` on the line above the \
         handler.",
        failures.join("\n  "),
    );
    assert!(
        markers.len() <= MAX_MARKERS,
        "{} handlers carry `{MARKER}` markers (limit {MAX_MARKERS}):\n  {}\n\n\
         Prefer a real gate over another exemption.",
        markers.len(),
        markers.join("\n  "),
    );
}
