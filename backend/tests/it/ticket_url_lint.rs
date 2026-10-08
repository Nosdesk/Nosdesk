//! Lint: a ticket's link is built by `utils::ticket_link`, never formatted by
//! hand.
//!
//! The web apps route a ticket by its number (`/tickets/{number}`) and look a
//! number up from an id at `/tickets/id/{id}`. A hand-formatted
//! `/tickets/{x}` is where an id has landed in place of a number before
//! (#598; the notification and search fallbacks fixed with this lint). So a
//! `format!` or `concat!` whose text holds a `/tickets/` route, or a string
//! concatenated onto `"/tickets/`, fails outside `ticket_link.rs`. Route
//! tables (`.route("/tickets/{id}", ...)`) are plain literals and don't
//! match; neither do storage paths (`/uploads/tickets/...`) or API paths
//! (`/api/tickets/...`). Test modules are skipped.

use std::fs;
use std::path::{Path, PathBuf};

const BUILDER: &str = "utils/ticket_link.rs";

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The file without its trailing test module.
fn without_tests(src: &str) -> &str {
    let mut at = 0;
    while let Some(i) = src[at..].find("#[cfg(test)]") {
        let start = at + i;
        let rest = src[start + "#[cfg(test)]".len()..].trim_start();
        if rest.starts_with("mod ") {
            return &src[..start];
        }
        at = start + 1;
    }
    src
}

/// The argument text of each `format!(`/`concat!(` call, with its byte offset.
fn macro_args(src: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    for name in ["format!(", "concat!("] {
        let mut at = 0;
        while let Some(i) = src[at..].find(name) {
            let start = at + i + name.len();
            let mut depth = 1;
            let mut end = start;
            let mut in_str = false;
            let bytes = src.as_bytes();
            while end < bytes.len() && depth > 0 {
                let c = bytes[end];
                if in_str {
                    if c == b'\\' {
                        end += 1;
                    } else if c == b'"' {
                        in_str = false;
                    }
                } else if c == b'"' {
                    in_str = true;
                } else if c == b'(' {
                    depth += 1;
                } else if c == b')' {
                    depth -= 1;
                }
                end += 1;
            }
            out.push((at + i, &src[start..end.saturating_sub(1)]));
            at = start;
        }
    }
    out
}

/// A `/tickets/` route in `text`, as opposed to a storage or API path.
fn names_a_ticket_route(text: &str) -> bool {
    text.match_indices("/tickets/").any(|(i, _)| {
        let before = &text[..i];
        !(before.ends_with("uploads") || before.ends_with("/api"))
    })
}

fn offences_in(src: &str) -> Vec<usize> {
    let src = without_tests(src);
    let line_of = |offset: usize| src[..offset].matches('\n').count() + 1;
    let mut lines: Vec<usize> = macro_args(src)
        .into_iter()
        .filter(|(_, args)| names_a_ticket_route(args))
        .map(|(offset, _)| line_of(offset))
        .collect();
    for (offset, _) in src.match_indices("\"/tickets/") {
        let tail = &src[offset + 1..];
        let literal_end = tail.find('"').map(|e| offset + 1 + e + 1);
        if let Some(end) = literal_end {
            if src[end..].trim_start().starts_with('+') {
                lines.push(line_of(offset));
            }
        }
    }
    lines.sort_unstable();
    lines.dedup();
    lines
}

#[test]
fn ticket_links_come_from_the_builder() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    files.sort();

    let mut offences = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel == BUILDER {
            continue;
        }
        for line in offences_in(&fs::read_to_string(path).unwrap()) {
            offences.push(format!("  src/{rel}:{line}"));
        }
    }
    assert!(
        offences.is_empty(),
        "A ticket link formatted by hand; build it with utils::ticket_link \
         (ticket_route / ticket_route_by_id / ticket_route_for):\n{}",
        offences.join("\n")
    );
}

#[test]
fn the_lint_tells_routes_from_paths() {
    assert_eq!(
        offences_in(r#"let u = format!("{base}/tickets/{n}");"#),
        vec![1]
    );
    assert_eq!(
        offences_in(r#"let u = format!("/tickets/id/{id}");"#),
        vec![1]
    );
    assert_eq!(offences_in("let u = \"/tickets/\" + &n;"), vec![1]);
    assert!(offences_in(r#"let p = format!("/uploads/tickets/{name}");"#).is_empty());
    assert!(offences_in(r#"let p = format!("{base}/api/tickets/{id}");"#).is_empty());
    assert!(offences_in(r#".route("/tickets/{id}", web::get().to(get))"#).is_empty());
    assert!(
        offences_in("#[cfg(test)]\nmod tests { fn t() { format!(\"/tickets/{id}\"); } }")
            .is_empty()
    );
}
