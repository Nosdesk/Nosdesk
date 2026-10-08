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
//! (`/api/tickets/...`). Test modules, inline or in their own files, are
//! skipped.

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

/// The byte just past the `}` closing the block opened at `open`, skipping
/// braces inside strings, chars and comments.
fn block_end(b: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b'r' if matches!(b.get(i + 1), Some(b'"' | b'#'))
                && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) =>
            {
                let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                if b.get(i + 1 + hashes) == Some(&b'"') {
                    let close: Vec<u8> = std::iter::once(b'"')
                        .chain(std::iter::repeat_n(b'#', hashes))
                        .collect();
                    let body = i + 2 + hashes;
                    i = b[body..]
                        .windows(close.len())
                        .position(|w| w == close.as_slice())
                        .map_or(b.len(), |p| body + p + close.len() - 1);
                }
            }
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'\'' if b.get(i + 1) == Some(&b'\\') => {
                i += 2;
                while i < b.len() && b[i] != b'\'' {
                    i += 1;
                }
            }
            b'\'' if b.get(i + 2) == Some(&b'\'') => i += 2,
            _ => {}
        }
        i += 1;
    }
    None
}

/// After a `#[cfg(test)]` ending at `at`: the module it gates, as its name
/// and whether its body is inline (`{` follows) or in its own file (`;`).
fn gated_module(src: &str, mut at: usize) -> Option<(&str, usize, bool)> {
    let b = src.as_bytes();
    loop {
        while at < b.len() && b[at].is_ascii_whitespace() {
            at += 1;
        }
        if src[at..].starts_with("#[") {
            at = block_end_square(b, at)?;
        } else {
            break;
        }
    }
    let rest = &src[at..];
    let rest = rest
        .strip_prefix("pub(crate) ")
        .or_else(|| rest.strip_prefix("pub "))
        .unwrap_or(rest);
    let name_start = src.len() - rest.strip_prefix("mod ")?.len();
    let name_len = src[name_start..]
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(0);
    let name = &src[name_start..name_start + name_len];
    let after = src[name_start + name_len..].trim_start();
    let delim = src.len() - after.len();
    match after.as_bytes().first() {
        Some(b'{') => Some((name, delim, true)),
        Some(b';') => Some((name, delim, false)),
        _ => None,
    }
}

/// The byte just past the `]` closing the attribute opened at `at`.
fn block_end_square(b: &[u8], at: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, &c) in b.iter().enumerate().skip(at) {
        match c {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The file with each inline `#[cfg(test)]` module blanked out (line numbers
/// kept), and the names of the test modules it keeps in their own files.
fn without_tests(src: &str) -> (String, Vec<String>) {
    const GATE: &str = "#[cfg(test)]";
    let mut out = src.as_bytes().to_vec();
    let mut external = Vec::new();
    for (start, _) in src.match_indices(GATE) {
        match gated_module(src, start + GATE.len()) {
            Some((_, open, true)) => {
                if let Some(end) = block_end(src.as_bytes(), open) {
                    for byte in &mut out[start..end] {
                        if *byte != b'\n' {
                            *byte = b' ';
                        }
                    }
                }
            }
            Some((name, _, false)) => external.push(name.to_string()),
            None => {}
        }
    }
    (
        String::from_utf8(out).expect("blanking keeps UTF-8"),
        external,
    )
}

/// Where the test modules `file` declares out of line live: `name.rs` and
/// the `name/` directory beside it.
fn test_module_files(file: &Path, names: &[String]) -> Vec<PathBuf> {
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let dir = if matches!(stem, "mod" | "lib" | "main") {
        file.parent().unwrap().to_path_buf()
    } else {
        file.with_extension("")
    };
    names
        .iter()
        .flat_map(|n| [dir.join(format!("{n}.rs")), dir.join(n)])
        .collect()
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
    let (src, _) = without_tests(src);
    let src = src.as_str();
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

    // Test modules kept in their own files are test code throughout.
    let test_files: Vec<PathBuf> = files
        .iter()
        .flat_map(|f| test_module_files(f, &without_tests(&fs::read_to_string(f).unwrap()).1))
        .collect();

    let mut offences = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel == BUILDER || test_files.iter().any(|t| path.starts_with(t)) {
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

/// Only the gated module is skipped: an out-of-line test module, or an inline
/// one, early in a file leaves the code after it checked.
#[test]
fn the_lint_skips_only_test_modules() {
    let src = "#[cfg(test)]\nmod helpers;\n\n#[cfg(test)]\n#[allow(dead_code)]\nmod tests {\n    fn t() { let _ = \"}\"; format!(\"/tickets/{id}\"); }\n}\n\nfn live(n: i32) -> String {\n    format!(\"/tickets/{n}\")\n}\n";
    assert_eq!(offences_in(src), vec![11]);
    assert_eq!(without_tests(src).1, vec!["helpers".to_string()]);
}
