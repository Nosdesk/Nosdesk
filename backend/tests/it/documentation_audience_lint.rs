//! Lint: who may open a documentation page or collection is decided in one
//! place, `repository::documentation::PageAudience`.
//!
//! Four copies of the rule once lived side by side (a single-page check, a
//! batch filter, a collection check and an inline filter in the collection
//! list), and the sync feed, the bootstrap and the REST routes each picked
//! one. A rule that changed in one copy and not the others shows a record to
//! someone in one place and hides it in another. Every caller now asks a
//! `PageAudience`, and this keeps a new copy from appearing.
//!
//! - The rule's helpers are named only inside `repository/documentation.rs`.
//! - The visibility tables are read only by the two documentation
//!   repositories (the rule itself, and the routes that list or set the
//!   grants).
//! - No other documentation file resolves the caller's groups, which is the
//!   first step of every copy of the rule.

#![allow(clippy::expect_used)]

use regex::Regex;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

const RULE_FILE: &str = "repository/documentation.rs";

/// Files that may name the visibility tables.
const TABLE_READERS: &[&str] = &[
    "repository/documentation.rs",
    "repository/documentation_collections.rs",
    // Diesel models and the generated join rules.
    "models/documentation.rs",
    "models/documentation_collections.rs",
    "schema_joins.rs",
];

#[test]
fn documentation_visibility_is_decided_by_page_audience() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let helpers = Regex::new(
        r"\b(can_user_access_page|filter_pages_for_user|can_user_access_collection|hidden_documentation_ids)\b",
    )
    .expect("regex");
    let tables =
        Regex::new(r"\b(documentation_page_visibility|documentation_collection_visibility)\b")
            .expect("regex");
    let groups = Regex::new(r"\bget_group_ids_for_users?\b").expect("regex");

    let mut stray = Vec::new();
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
        if relpath == "schema.rs" || relpath == RULE_FILE {
            continue;
        }
        let src = fs::read_to_string(entry.path()).expect("read source");
        for m in helpers.find_iter(&src) {
            stray.push(format!("{relpath}: names `{}`", m.as_str()));
        }
        if !TABLE_READERS.contains(&relpath.as_str()) {
            for m in tables.find_iter(&src) {
                stray.push(format!("{relpath}: reads `{}`", m.as_str()));
            }
        }
        if relpath.contains("documentation") {
            for m in groups.find_iter(&src) {
                stray.push(format!("{relpath}: resolves groups with `{}`", m.as_str()));
            }
        }
    }

    assert!(
        stray.is_empty(),
        "documentation visibility decided outside PageAudience; ask a \
         `repository::PageAudience` instead:\n  {}",
        stray.join("\n  ")
    );
}
