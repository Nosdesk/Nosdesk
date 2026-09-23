//! Lint: a read of `workspace_members` says whether it wants active members.
//!
//! `workspace_members` rows outlive the membership. `remove_membership` stamps
//! `removed_at` rather than deleting, because the row is what lets a former
//! colleague's name still render on the tickets, comments and audit entries
//! they left behind. That makes every read ambiguous in a way it was not
//! before:
//!
//! - **Role, permission and seat reads** must filter `removed_at IS NULL`.
//!   Forgetting leaves a removed member holding their role, or consuming a
//!   licensed seat forever.
//! - **Identity resolution** must NOT filter, or the history the column exists
//!   to preserve renders as an unknown user again.
//!
//! Both are one-line changes and neither fails loudly, so the mistake is
//! invisible in review and in production. This makes the choice explicit.
//!
//! ## Escape hatch
//!
//! A read that deliberately spans removed members carries, on the line above
//! the query or its enclosing `pub fn`:
//!
//! ```ignore
//! // members-any-status: <why this read includes removed members>
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Files whose `workspace_members` use is not a tenant read at all.
const SKIP: &[&str] = &["schema.rs", "test_helpers.rs"];

/// Everything from the first `#[cfg(test)]` onward. Test modules build and tear
/// down membership rows directly, which is not the behaviour under review;
/// the other lints in this directory drop them the same way.
fn strip_test_modules(src: &str) -> &str {
    match src.find("#[cfg(test)]") {
        Some(i) => &src[..i],
        None => src,
    }
}

const MARKER: &str = "members-any-status:";

#[derive(Debug)]
struct Violation {
    relpath: String,
    line: usize,
    snippet: String,
}

fn scan(root: &Path, out: &mut Vec<Violation>) {
    let table_re = Regex::new(r"workspace_members::table|FROM workspace_members").expect("re");
    // The filter can be Diesel or raw SQL, and may sit several lines below the
    // table reference, so the window is the query, not the line.
    let filtered_re = Regex::new(r"removed_at\s*(?:\.is_null\(\)|IS NULL)").expect("re");

    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
    {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if SKIP.contains(&name) {
            continue;
        }
        let Ok(src) = fs::read_to_string(path) else {
            continue;
        };
        let lines: Vec<&str> = strip_test_modules(&src).lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !table_re.is_match(line) {
                continue;
            }
            // A query is at most ~18 lines here; look that far for the filter.
            let end = (i + 18).min(lines.len());
            let window = lines[i..end].join("\n");
            if filtered_re.is_match(&window) {
                continue;
            }
            // The marker may be above the query or above its enclosing fn.
            let back = i.saturating_sub(25);
            if lines[back..i].iter().any(|l| l.contains(MARKER)) {
                continue;
            }
            out.push(Violation {
                relpath: path
                    .strip_prefix(root.parent().unwrap_or(root))
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                line: i + 1,
                snippet: line.trim().chars().take(70).collect(),
            });
        }
    }
}

#[test]
fn workspace_members_reads_state_whether_they_want_active_members() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_root = manifest.join("src");
    assert!(src_root.exists());

    let mut violations = Vec::new();
    scan(&src_root, &mut violations);

    if !violations.is_empty() {
        let mut msg = String::from(
            "\nThese reads of `workspace_members` neither filter `removed_at IS NULL`\n\
             nor say that they mean to include removed members.\n\n\
             A removed membership row still exists, so an unfiltered role, permission\n\
             or seat read silently treats a removed person as a current one.\n\n\
             Add `.filter(workspace_members::removed_at.is_null())` (or `AND removed_at\n\
             IS NULL` in raw SQL), or, if the read genuinely spans removed members —\n\
             identity resolution for historical records is the reason this column\n\
             exists — put this directly above the query or its enclosing fn:\n\n  \
             // members-any-status: <why>\n\n\
             Sites:\n\n",
        );
        for v in &violations {
            msg.push_str(&format!("  {}:{}  {}\n", v.relpath, v.line, v.snippet));
        }
        panic!("{msg}");
    }
}
