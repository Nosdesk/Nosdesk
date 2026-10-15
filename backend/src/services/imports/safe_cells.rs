//! Spreadsheet formula-injection guard for CSV cells, and its reverse for
//! files coming back in.

/// Neutralize spreadsheet formula injection. Excel and Sheets treat a
/// cell as a formula when its first non-whitespace character is `=`,
/// `+`, `-`, or `@`. Some importers strip leading whitespace (space,
/// tab, CR, LF, NUL) before evaluating, so a payload like "\t=cmd"
/// would slip past a first-byte-only check; look past the leading
/// whitespace before deciding. Prefix with `'` so the value opens as
/// plain text. The `csv` crate already quotes commas and newlines;
/// this is separate hardening. See security-audit-2026-06.
pub fn csv_formula_safe(value: String) -> String {
    if formula_trigger(&value) {
        format!("'{value}")
    } else {
        value
    }
}

/// Undo `csv_formula_safe` on an imported cell: a `'` followed by what
/// the guard would have prefixed loses the `'`, so an export imports
/// back as the values it was written from. Any other cell is untouched.
pub fn strip_formula_guard(value: &str) -> &str {
    match value.strip_prefix('\'') {
        Some(rest) if formula_trigger(rest) => rest,
        _ => value,
    }
}

fn formula_trigger(value: &str) -> bool {
    let leading = value.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{0}');
    matches!(leading.chars().next(), Some('=' | '+' | '-' | '@'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_guard_handles_leading_whitespace_and_controls() {
        // Direct triggers.
        assert_eq!(csv_formula_safe("=cmd".into()), "'=cmd");
        assert_eq!(csv_formula_safe("@SUM".into()), "'@SUM");
        // Leading whitespace / control chars before the trigger must
        // still be neutralised (some importers strip them first).
        assert_eq!(csv_formula_safe(" =cmd".into()), "' =cmd");
        assert_eq!(csv_formula_safe("\t=cmd".into()), "'\t=cmd");
        assert_eq!(csv_formula_safe("\r-cmd".into()), "'\r-cmd");
        // Benign values are untouched.
        assert_eq!(csv_formula_safe("Laptop".into()), "Laptop");
        assert_eq!(csv_formula_safe("  hello".into()), "  hello");
        assert_eq!(csv_formula_safe(String::new()), "");
    }

    #[test]
    fn guard_round_trips() {
        for value in [
            "=SUM(A1)",
            "+61 2 9999 0000",
            "-5",
            "@handle",
            " =cmd",
            "\t=cmd",
            "\u{0}@x",
            "Laptop",
            "it's fine",
            "'",
            "",
        ] {
            let guarded = csv_formula_safe(value.to_string());
            assert_eq!(strip_formula_guard(&guarded), value, "{value:?}");
        }
    }

    #[test]
    fn strip_leaves_other_quoted_cells() {
        assert_eq!(strip_formula_guard("'quoted"), "'quoted");
        assert_eq!(strip_formula_guard("'"), "'");
        assert_eq!(strip_formula_guard("O'Brien"), "O'Brien");
        assert_eq!(strip_formula_guard("' =x"), " =x");
        assert_eq!(strip_formula_guard("'-12.5"), "-12.5");
    }
}
