//! Lint: a portal handler reaches tickets only through
//! `PortalContext::visibility` / `own_visibility`.
//!
//! Those two fold in the session's scope (a session opened from one ticket's
//! link reaches that ticket alone). A handler that built its own
//! `VisibilityContext` would read past it. In any file that uses
//! `PortalContext`, the portal constructors (`requester_only`,
//! `portal_sharing`, `limited_to`, `shared_visibility`) may appear only inside
//! the builder: `impl PortalContext` and `fn shared_visibility` in
//! `handlers/portal.rs`.

#![allow(clippy::expect_used)]

use regex::Regex;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

/// `src` with the brace-delimited block that starts at the first match of
/// `head` removed.
fn without_block(src: &str, head: &str) -> String {
    let Some(start) = src.find(head) else {
        return src.to_string();
    };
    let open = start + src[start..].find('{').expect("block opens");
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return format!("{}{}", &src[..start], &src[open + i + 1..]);
                }
            }
            _ => {}
        }
    }
    panic!("block after `{head}` never closes");
}

#[test]
fn portal_handlers_reach_tickets_only_through_the_builder() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let constructors =
        Regex::new(r"\b(requester_only|portal_sharing|limited_to|shared_visibility)\s*\(")
            .expect("regex");

    let mut stray = Vec::new();
    let mut builder_seen = false;
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
        let mut src = fs::read_to_string(entry.path()).expect("read source");
        if !src.contains("PortalContext") {
            continue;
        }
        if relpath == "handlers/portal.rs" {
            builder_seen = src.contains("impl PortalContext {");
            src = without_block(&src, "impl PortalContext {");
            src = without_block(&src, "fn shared_visibility(");
        }
        for m in constructors.find_iter(&src) {
            stray.push(format!("{relpath}: calls `{}`", m.as_str()));
        }
    }

    assert!(
        builder_seen,
        "handlers/portal.rs no longer has `impl PortalContext`"
    );
    assert!(
        stray.is_empty(),
        "A portal handler built its own ticket visibility, which skips the session's \
         scope. Use `portal.visibility(conn)` or `portal.own_visibility(conn)`:\n  {}",
        stray.join("\n  ")
    );
}
