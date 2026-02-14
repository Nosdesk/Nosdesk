use crate::db::DbConnection;
use crate::repository::documentation::get_documentation_page_by_slug;

/// Generate a unique slug from a title for documentation pages.
///
/// - Converts to lowercase, replaces non-alphanumeric chars with hyphens
/// - Collapses consecutive hyphens, trims leading/trailing hyphens
/// - Prefixes purely numeric slugs with "page-" to avoid ambiguity with ID routing
/// - Appends -2, -3, etc. if the slug already exists
pub fn generate_unique_slug(title: &str, conn: &mut DbConnection) -> String {
    let base = slugify(title);
    let base = if base.is_empty() {
        "untitled".to_string()
    } else {
        base
    };

    // Check if base slug is available
    if get_documentation_page_by_slug(&base, conn).is_err() {
        return base;
    }

    // Collision — try suffixes -2, -3, ...
    for n in 2..1000 {
        let candidate = format!("{}-{}", base, n);
        if get_documentation_page_by_slug(&candidate, conn).is_err() {
            return candidate;
        }
    }

    // Fallback: append timestamp (should never happen in practice)
    format!("{}-{}", base, chrono::Utc::now().timestamp())
}

/// Convert a title string into a URL-safe slug.
fn slugify(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();

    // Collapse consecutive hyphens and trim
    let slug = collapse_hyphens(&slug);
    let slug = slug.trim_matches('-').to_string();

    // If the slug is purely numeric, prefix with "page-" to avoid ambiguity
    if !slug.is_empty() && slug.chars().all(|c| c.is_ascii_digit()) {
        format!("page-{}", slug)
    } else {
        slug
    }
}

fn collapse_hyphens(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_hyphen = false;
    for c in s.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push('-');
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn basic_title() {
        assert_eq!(slugify("Getting Started"), "getting-started");
    }

    #[test]
    fn special_characters() {
        assert_eq!(slugify("Hello, World! (2024)"), "hello-world-2024");
    }

    #[test]
    fn consecutive_spaces() {
        assert_eq!(slugify("a   b"), "a-b");
    }

    #[test]
    fn leading_trailing() {
        assert_eq!(slugify("  hello  "), "hello");
    }

    #[test]
    fn numeric_title() {
        assert_eq!(slugify("404"), "page-404");
    }

    #[test]
    fn empty_title() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn unicode_stripped() {
        assert_eq!(slugify("café résumé"), "caf-r-sum");
    }
}
