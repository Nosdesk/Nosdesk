//! Shared slug validator for workspace-creation surfaces.
//!
//! Two callers exercise the same rules:
//!   * `handlers::internal_workspaces::create_workspace` — the
//!     M5 control-plane provisioning callback.
//!   * `handlers::admin_workspaces::create_workspace` — the
//!     Phase 4 W1 admin / platform-admin surface.
//!
//! Stricter than the DB CHECK (`^[a-z0-9](...){0,62}[a-z0-9]$`):
//!   * must start with a letter (DB allows digits)
//!   * must end with letter or digit (no trailing hyphen)
//!   * no consecutive hyphens (DB doesn't enforce)
//!   * 3 to 40 chars (DB allows 1 to 64)
//!   * not in [`reserved_slugs`] (the W4 denylist also enforced
//!     by the workspaces_slug_not_reserved CHECK)

use once_cell::sync::Lazy;
use regex::Regex;

use crate::utils::reserved_slugs;

static SLUG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z][a-z0-9-]*[a-z0-9]$").expect("compile slug regex"));

#[derive(Debug, PartialEq, Eq)]
pub enum SlugError {
    BadLength,
    BadShape,
    ConsecutiveHyphens,
    Reserved,
}

impl SlugError {
    pub fn as_message(&self) -> &'static str {
        match self {
            Self::BadLength => "slug must be 3 to 40 characters",
            Self::BadShape => {
                "slug must be lowercase letters, digits, and hyphens; start with a letter and end with a letter or digit"
            }
            Self::ConsecutiveHyphens => "slug must not contain consecutive hyphens",
            Self::Reserved => "slug is reserved, please choose another",
        }
    }
}

pub fn validate_slug(slug: &str) -> Result<(), SlugError> {
    if !(3..=40).contains(&slug.len()) {
        return Err(SlugError::BadLength);
    }
    if !SLUG_RE.is_match(slug) {
        return Err(SlugError::BadShape);
    }
    if slug.contains("--") {
        return Err(SlugError::ConsecutiveHyphens);
    }
    if reserved_slugs::is_reserved(slug) {
        return Err(SlugError::Reserved);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_slugs() {
        for s in [
            "abc",
            "acme",
            "abc-co",
            "abc1",
            "a-1-b",
            "twelve34five-6789",
        ] {
            assert!(validate_slug(s).is_ok(), "expected {s} to be valid");
        }
    }

    #[test]
    fn rejects_bad_length() {
        assert!(matches!(validate_slug("ab"), Err(SlugError::BadLength)));
        let too_long = "a".repeat(41);
        assert!(matches!(
            validate_slug(&too_long),
            Err(SlugError::BadLength)
        ));
    }

    #[test]
    fn rejects_bad_shape() {
        for s in [
            "1abc",    // starts with digit
            "abc-",    // ends with hyphen
            "-abc",    // starts with hyphen
            "ABC",     // uppercase
            "abc_def", // underscore
            "abc def", // space
        ] {
            assert!(
                matches!(validate_slug(s), Err(SlugError::BadShape)),
                "expected {s} to fail shape rule"
            );
        }
    }

    #[test]
    fn rejects_consecutive_hyphens() {
        assert!(matches!(
            validate_slug("abc--def"),
            Err(SlugError::ConsecutiveHyphens)
        ));
    }

    #[test]
    fn rejects_reserved_slugs() {
        for s in ["api", "app", "www", "admin", "staging"] {
            assert!(
                matches!(validate_slug(s), Err(SlugError::Reserved)),
                "expected {s} to be reserved"
            );
        }
    }
}
