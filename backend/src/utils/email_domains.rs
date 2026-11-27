//! An organisation's email domains as an admin lists them: for the requester
//! sign-in provider (`handlers::requester_sso`) and for the workspace's own list
//! (`site_settings.organisation_domains`).

use crate::errors::ApiError;

/// The most domains one list may hold.
pub const MAX_DOMAINS: usize = 50;

/// Lowercased, trimmed, without a leading `@`, de-duplicated domains, in the
/// order given. Blank entries are skipped, so an empty list is valid here; a
/// caller that needs one says so. Refuses anything that isn't a domain, a
/// free-mail domain (it would stand for anyone with a personal account), and a
/// list longer than [`MAX_DOMAINS`].
pub fn clean_domains(domains: &[String]) -> Result<Vec<String>, ApiError> {
    let mut out: Vec<String> = Vec::new();
    for raw in domains {
        let d = raw.trim().trim_start_matches('@').to_lowercase();
        if d.is_empty() {
            continue;
        }
        let valid = d.contains('.')
            && d.len() <= 253
            && d.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
        if !valid {
            return Err(ApiError::BadRequest(format!("{d} isn't a domain")));
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
