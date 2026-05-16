//! Fixture-driven regression suite for the email quote splitter.
//!
//! Iterates `backend/tests/fixtures/email_quote/` and asserts the
//! splitter's output matches the expected files for each case.
//! See the directory README for the fixture format and how to add
//! a new case.
//!
//! The harness picks the plaintext or HTML splitter based on the
//! `input` file's extension (`.txt` or `.html`) so the same
//! directory can hold cases for both paths without separate
//! subtrees.

use std::fs;
use std::path::{Path, PathBuf};

use backend::services::channels::email_quote::{split_html, split_plaintext, QuoteSplit};

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("email_quote")
}

/// Walk the fixtures directory, run the splitter on each case,
/// and report any mismatches as part of a single test failure.
/// Collecting failures rather than panicking on the first lets a
/// CI run surface every regression at once, which matters when
/// the corpus grows.
#[test]
fn corpus_split_matches_expected() {
    let root = fixtures_root();
    assert!(root.is_dir(), "fixture root missing: {}", root.display());

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for entry in fs::read_dir(&root).expect("read fixture root") {
        let case_dir = entry.expect("dir entry").path();
        if !case_dir.is_dir() {
            continue;
        }
        let name = case_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>")
            .to_string();

        match run_case(&case_dir) {
            Ok(()) => checked += 1,
            Err(msg) => failures.push(format!("[{name}] {msg}")),
        }
    }

    assert!(checked > 0, "no fixtures executed; corpus is empty?");
    assert!(
        failures.is_empty(),
        "{} fixture failure(s):\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Run a single fixture directory. Returns `Err(message)` when
/// the case mismatched so the caller can collect failures.
fn run_case(case_dir: &Path) -> Result<(), String> {
    let (input_path, ext) =
        find_input(case_dir).ok_or_else(|| "missing input.{txt,html}".to_string())?;

    let input = fs::read_to_string(&input_path).map_err(|e| format!("read input: {e}"))?;

    let expected_new_path = case_dir.join(format!("expected_new.{ext}"));
    let expected_quoted_path = case_dir.join(format!("expected_quoted.{ext}"));

    let expected_new = fs::read_to_string(&expected_new_path)
        .map_err(|e| format!("read expected_new.{ext}: {e}"))?;
    let expected_quoted = match fs::read_to_string(&expected_quoted_path) {
        Ok(s) => Some(s),
        Err(_) => None,
    };

    let actual: QuoteSplit = if ext == "html" {
        split_html(&input)
    } else {
        split_plaintext(&input)
    };

    let expected_new_norm = expected_new.trim_end_matches(['\n', '\r']).to_string();
    let expected_quoted_norm =
        expected_quoted.map(|s| s.trim_end_matches(['\n', '\r']).to_string());

    let mut diffs: Vec<String> = Vec::new();

    if actual.new_content != expected_new_norm {
        diffs.push(format!(
            "new_content mismatch\n  expected:\n{}\n  actual:\n{}",
            indent(&expected_new_norm),
            indent(&actual.new_content),
        ));
    }

    let actual_quoted_norm = actual
        .quoted_content
        .as_deref()
        .map(|s| s.trim_end_matches(['\n', '\r']).to_string());

    if actual_quoted_norm != expected_quoted_norm {
        diffs.push(format!(
            "quoted_content mismatch\n  expected: {:?}\n  actual:   {:?}",
            expected_quoted_norm, actual_quoted_norm
        ));
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        Err(diffs.join("\n"))
    }
}

fn find_input(case_dir: &Path) -> Option<(PathBuf, &'static str)> {
    for ext in &["txt", "html"] {
        let candidate = case_dir.join(format!("input.{ext}"));
        if candidate.is_file() {
            return Some((candidate, ext));
        }
    }
    None
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
