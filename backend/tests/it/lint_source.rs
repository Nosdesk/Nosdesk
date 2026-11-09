//! Source reading shared by the lints that look for writes in `src/`: the
//! files to scan, code with comments and strings masked, and which code is
//! test code.

#![allow(clippy::expect_used)]

use std::collections::HashSet;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use regex::Regex;
use walkdir::WalkDir;

/// Every `.rs` file under `src/`, as (path relative to `src/`, contents).
pub(crate) fn rust_sources() -> Vec<(String, String)> {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    WalkDir::new(&src_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
        .map(|e| {
            let rel = relative(&src_root, e.path());
            (rel, fs::read_to_string(e.path()).expect("read source"))
        })
        .collect()
}

/// `src` with comments blanked, and string and char literal contents blanked
/// too when `strings` is set. Byte offsets and line breaks are kept, so a match
/// in the result points at the same place in `src`.
pub(crate) fn mask(src: &str, strings: bool) -> String {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let blank = |out: &mut Vec<u8>, range: Range<usize>| {
        for i in range {
            if out[i] != b'\n' {
                out[i] = b' ';
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                let end = src[i..].find('\n').map_or(b.len(), |e| i + e);
                blank(&mut out, i..end);
                i = end;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut depth = 0;
                let mut j = i;
                while j < b.len() {
                    if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                        depth += 1;
                        j += 2;
                    } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                        depth -= 1;
                        j += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        j += 1;
                    }
                }
                blank(&mut out, i..j.min(b.len()));
                i = j;
            }
            b'r' if !ident_before(b, i) && matches!(b.get(i + 1), Some(b'"' | b'#')) => {
                // Raw string: r"..." or r#"..."#.
                let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                let open = i + 1 + hashes;
                if b.get(open) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let close = format!("\"{}", "#".repeat(hashes));
                let end = src[open + 1..]
                    .find(&close)
                    .map_or(b.len(), |e| open + 1 + e);
                if strings {
                    blank(&mut out, open + 1..end);
                }
                i = end + close.len();
            }
            b'"' => {
                let mut j = i + 1;
                while j < b.len() && b[j] != b'"' {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                if strings {
                    blank(&mut out, i + 1..j.min(b.len()));
                }
                i = j + 1;
            }
            b'\'' => {
                // A char literal ('x', '\n', '\u{1F600}'), not a lifetime.
                let end = if b.get(i + 1) == Some(&b'\\') {
                    src[i + 2..].find('\'').map(|e| i + 2 + e)
                } else {
                    let len = src[i + 1..].chars().next().map_or(1, char::len_utf8);
                    (b.get(i + 1 + len) == Some(&b'\'')).then_some(i + 1 + len)
                };
                match end {
                    Some(end) => {
                        if strings {
                            blank(&mut out, i + 1..end);
                        }
                        i = end + 1;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).expect("masking keeps UTF-8 boundaries")
}

fn ident_before(b: &[u8], i: usize) -> bool {
    i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')
}

/// The byte ranges of `#[cfg(test)]` items in masked code: from the attribute
/// to the brace that closes the item's body. An item without a body (`mod
/// name;`) has no range here; its file is found by `test_only_files`.
pub(crate) fn test_regions(code: &str) -> Vec<Range<usize>> {
    let b = code.as_bytes();
    let mut regions = Vec::new();
    for (start, _) in code.match_indices("#[cfg(test)]") {
        let after = start + "#[cfg(test)]".len();
        let Some(open) = code[after..].find(['{', ';']).map(|o| after + o) else {
            continue;
        };
        if b[open] == b';' {
            continue;
        }
        let mut depth = 0usize;
        let mut end = b.len();
        for (j, &c) in b.iter().enumerate().skip(open) {
            match c {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = j + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        regions.push(start..end);
    }
    regions
}

/// Files that are test modules by their declaration: `#[cfg(test)] mod name;`
/// in a parent, resolved to `name.rs` or `name/mod.rs` beside it.
pub(crate) fn test_only_files(sources: &[(String, String)]) -> HashSet<String> {
    let decl = Regex::new(r"#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;")
        .expect("module declaration pattern");
    let known: HashSet<&str> = sources.iter().map(|(rel, _)| rel.as_str()).collect();
    let mut out = HashSet::new();
    for (rel, src) in sources {
        let path = Path::new(rel);
        let dir = match path.file_stem().and_then(|s| s.to_str()) {
            Some("mod" | "lib" | "main") => path.parent().map(Path::to_path_buf),
            _ => Some(path.with_extension("")),
        }
        .unwrap_or_default();
        for caps in decl.captures_iter(&mask(src, true)) {
            let name = &caps[1];
            for candidate in [
                dir.join(format!("{name}.rs")),
                dir.join(name).join("mod.rs"),
            ] {
                let candidate = candidate.to_string_lossy().replace('\\', "/");
                if known.contains(candidate.as_str()) {
                    out.insert(candidate);
                }
            }
        }
    }
    out
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("under src")
        .to_string_lossy()
        .replace('\\', "/")
}
