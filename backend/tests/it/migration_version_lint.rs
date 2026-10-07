//! Lint: no two migration directories share a version.
//!
//! Diesel's version is a directory name up to its first `_`, with dashes
//! removed. `pending_migrations` keys a map by that version, so when two
//! directories share one, one of them never runs, on a fresh database or an
//! upgrade, and nothing reports it. Name new migrations with
//! `date -u +%Y-%m-%d-%H%M%S`; `00000000000000` is diesel's initial version.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Diesel's version for a migration directory (`version_from_string` in
/// migrations_internals).
fn version(dir_name: &str) -> String {
    dir_name
        .split('_')
        .next()
        .unwrap_or(dir_name)
        .replace('-', "")
}

/// Every directory `embed_migrations!` would embed from `root`, with the
/// problems found, if any.
fn check(root: &Path) -> Result<(), String> {
    let stamp = Regex::new(r"^(\d{4}-\d{2}-\d{2}-\d{6}|0{14})_").expect("stamp regex");
    let mut by_version: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut misnamed = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| format!("read {}: {e}", root.display()))? {
        let entry = entry.map_err(|e| format!("read {}: {e}", root.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // Same filter as embed_migrations!: skip hidden entries and files.
        let is_file = entry.metadata().map(|m| m.is_file()).unwrap_or(false);
        if name.starts_with('.') || is_file {
            continue;
        }
        if !stamp.is_match(&name) {
            misnamed.push(name.clone());
        }
        by_version.entry(version(&name)).or_default().push(name);
    }

    let mut problems = Vec::new();
    let shared: Vec<String> = by_version
        .iter()
        .filter(|(_, dirs)| dirs.len() > 1)
        .map(|(version, dirs)| {
            let mut dirs = dirs.clone();
            dirs.sort();
            format!("{version}: {}", dirs.join(", "))
        })
        .collect();
    if !shared.is_empty() {
        problems.push(format!(
            "these migrations share a version, so diesel runs only one of each \
             group and skips the rest without an error. Give each its own \
             `date -u +%Y-%m-%d-%H%M%S` prefix:\n  {}",
            shared.join("\n  ")
        ));
    }
    if !misnamed.is_empty() {
        misnamed.sort();
        problems.push(format!(
            "these migration directories don't start with a \
             `YYYY-MM-DD-HHMMSS_` version:\n  {}",
            misnamed.join("\n  ")
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

#[test]
fn migration_versions_are_unique() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations");
    if let Err(problems) = check(&root) {
        panic!("{problems}");
    }
}

/// The check itself catches a planted duplicate, including one spelled
/// without dashes, and ignores what diesel ignores.
#[test]
fn a_shared_version_is_reported() {
    let tmp = tempfile::tempdir().expect("tempdir");
    for dir in [
        "00000000000000_initial_schema",
        "2026-01-01-000000_first",
        "2026-01-01-000000_second",
        "20260101000000_third",
        "2026-01-02-000000_unrelated",
        ".hidden",
    ] {
        fs::create_dir(tmp.path().join(dir)).expect("create dir");
    }
    fs::write(tmp.path().join("README.md"), "").expect("create file");

    let problems = check(tmp.path()).expect_err("a shared version must fail");
    assert!(
        problems.contains(
            "20260101000000: 2026-01-01-000000_first, 2026-01-01-000000_second, \
             20260101000000_third"
        ),
        "{problems}"
    );
    assert!(
        problems.contains("YYYY-MM-DD-HHMMSS_` version:\n  20260101000000_third"),
        "{problems}"
    );
    assert!(!problems.contains("unrelated"), "{problems}");
    assert!(!problems.contains("initial_schema"), "{problems}");

    fs::remove_dir(tmp.path().join("2026-01-01-000000_second")).expect("remove dir");
    fs::remove_dir(tmp.path().join("20260101000000_third")).expect("remove dir");
    assert_eq!(check(tmp.path()), Ok(()));
}
