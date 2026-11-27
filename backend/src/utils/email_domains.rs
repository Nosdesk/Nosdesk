//! An organisation's email domains as an admin lists them: for the requester
//! sign-in provider (`handlers::requester_sso`) and for the workspace's own list
//! (`site_settings.organisation_domains`).

use crate::errors::ApiError;

/// The most domains one list may hold.
pub const MAX_DOMAINS: usize = 50;

/// Lowercased, trimmed, without a leading `@` or one trailing dot,
/// de-duplicated domains, in the order given. Blank entries are skipped, so an empty list is valid here; a
/// caller that needs one says so. Refuses anything that isn't a domain, a
/// free-mail domain (it would stand for anyone with a personal account), and a
/// list longer than [`MAX_DOMAINS`].
pub fn clean_domains(domains: &[String]) -> Result<Vec<String>, ApiError> {
    let mut out: Vec<String> = Vec::new();
    for raw in domains {
        let trimmed = raw.trim().trim_start_matches('@');
        if trimmed.is_empty() {
            continue;
        }
        // A fully qualified `acme.com.` is `acme.com`.
        let d = trimmed.strip_suffix('.').unwrap_or(trimmed).to_lowercase();
        if !is_domain(&d) {
            return Err(ApiError::BadRequest(format!(
                "{} isn't a domain",
                trimmed.to_lowercase()
            )));
        }
        if crate::utils::free_mail::is_free_mail(&d) {
            return Err(ApiError::BadRequest(format!(
                "{d} is a personal email provider; list your organisation's own domains"
            )));
        }
        if !out.contains(&d) {
            out.push(d);
        }
    }
    if out.len() > MAX_DOMAINS {
        return Err(ApiError::BadRequest(format!(
            "List at most {MAX_DOMAINS} domains"
        )));
    }
    Ok(out)
}

/// At least two labels, 253 characters at most; each label 1 to 63 letters,
/// digits and hyphens, not starting or ending with a hyphen.
fn is_domain(d: &str) -> bool {
    d.len() <= 253
        && d.contains('.')
        && d.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(domains: &[&str]) -> Vec<String> {
        domains.iter().map(|d| d.to_string()).collect()
    }

    fn message(result: Result<Vec<String>, ApiError>) -> String {
        match result {
            Err(ApiError::BadRequest(m)) => m,
            other => panic!("expected a bad request, got {other:?}"),
        }
    }

    #[test]
    fn domains_are_trimmed_lowercased_and_deduplicated() {
        assert_eq!(
            clean_domains(&list(&["@Acme.com ", "acme.com", " it.acme.com", "", "  "])).unwrap(),
            list(&["acme.com", "it.acme.com"])
        );
    }

    #[test]
    fn an_empty_list_is_valid() {
        assert_eq!(clean_domains(&[]).unwrap(), Vec::<String>::new());
        assert_eq!(
            clean_domains(&list(&["", " "])).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn free_mail_is_refused() {
        assert_eq!(
            message(clean_domains(&list(&["acme.com", "Gmail.com"]))),
            "gmail.com is a personal email provider; list your organisation's own domains"
        );
        assert!(clean_domains(&list(&["hotmail.fr"])).is_err());
    }

    #[test]
    fn anything_but_a_domain_is_refused() {
        assert_eq!(
            message(clean_domains(&list(&["not a domain"]))),
            "not a domain isn't a domain"
        );
        assert!(clean_domains(&list(&["localhost"])).is_err());
        assert!(clean_domains(&list(&["acme.com/path"])).is_err());
        assert!(clean_domains(&list(&["user@acme.com"])).is_err());
        assert!(clean_domains(&[format!("{}.com", "a".repeat(250))]).is_err());
    }

    #[test]
    fn one_trailing_dot_is_dropped() {
        assert_eq!(
            clean_domains(&list(&["acme.com.", "Acme.com"])).unwrap(),
            list(&["acme.com"])
        );
        assert!(clean_domains(&list(&["acme.com.."])).is_err());
    }

    #[test]
    fn labels_must_be_well_formed() {
        for bad in [
            ".",
            "..",
            ".acme.com",
            "a..com",
            "-acme.com",
            "acme-.com",
            "acme.-com",
            "acme.com-",
        ] {
            assert!(clean_domains(&list(&[bad])).is_err(), "{bad}");
        }
        let longest = "a".repeat(63);
        assert!(clean_domains(&[format!("{longest}.com")]).is_ok());
        assert!(clean_domains(&[format!("{longest}a.com")]).is_err());
        assert!(clean_domains(&list(&["it-desk.acme.com", "x1.co"])).is_ok());
    }

    #[test]
    fn the_list_is_capped() {
        let at_cap: Vec<String> = (0..MAX_DOMAINS).map(|i| format!("d{i}.example")).collect();
        assert_eq!(clean_domains(&at_cap).unwrap().len(), MAX_DOMAINS);
        let mut over = at_cap.clone();
        over.push("one-more.example".into());
        assert_eq!(message(clean_domains(&over)), "List at most 50 domains");
        // Duplicates don't count against the cap.
        let mut repeated = at_cap;
        repeated.push("d0.example".into());
        assert!(clean_domains(&repeated).is_ok());
    }
}
