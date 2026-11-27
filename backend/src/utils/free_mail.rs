//! Free-mail providers: an address at one of these says nothing about who the
//! person works for, so organisation visibility never groups people by it.

/// Consumer mailbox domains, lowercase.
const FREE_MAIL_DOMAINS: &[&str] = &[
    "aol.com",
    "bigpond.com",
    "fastmail.com",
    "gmail.com",
    "gmx.com",
    "gmx.de",
    "googlemail.com",
    "hey.com",
    "hotmail.co.uk",
    "hotmail.com",
    "icloud.com",
    "live.com",
    "mac.com",
    "mail.com",
    "me.com",
    "msn.com",
    "outlook.com",
    "pm.me",
    "proton.me",
    "protonmail.com",
    "qq.com",
    "tutanota.com",
    "web.de",
    "yahoo.co.uk",
    "yahoo.com",
    "yandex.ru",
    "ymail.com",
    "zoho.com",
];

/// Consumer providers that run a domain per country (`hotmail.fr`,
/// `yahoo.co.jp`, `gmx.net`): the first label, before a country or generic
/// suffix of at most two short labels.
const FREE_MAIL_FAMILIES: &[&str] = &[
    "gmx", "hotmail", "live", "outlook", "yahoo", "yandex", "ymail",
];

/// Whether `address` (or a bare domain) is at a free-mail provider.
pub fn is_free_mail(address: &str) -> bool {
    let domain = address
        .rsplit('@')
        .next()
        .unwrap_or(address)
        .trim()
        .trim_end_matches('.')
        .to_lowercase();
    if FREE_MAIL_DOMAINS.contains(&domain.as_str()) {
        return true;
    }
    // `hotmail.fr`, `yahoo.co.jp`, `live.com.au`, but not `live.acme.com`.
    let Some((first, suffix)) = domain.split_once('.') else {
        return false;
    };
    let labels: Vec<&str> = suffix.split('.').collect();
    FREE_MAIL_FAMILIES.contains(&first)
        && labels.len() <= 2
        && labels.iter().all(|l| !l.is_empty() && l.len() <= 3)
}

#[cfg(test)]
mod tests {
    use super::is_free_mail;

    #[test]
    fn consumer_domains_are_free_mail() {
        assert!(is_free_mail("someone@Gmail.com"));
        assert!(is_free_mail("outlook.com"));
        assert!(!is_free_mail("it@acme.com.au"));
    }

    #[test]
    fn a_providers_country_domains_are_free_mail() {
        for d in [
            "hotmail.fr",
            "yahoo.co.jp",
            "live.com.au",
            "gmx.net",
            "yandex.com",
            "ymail.com",
        ] {
            assert!(is_free_mail(d), "{d}");
        }
        for d in ["live.acme.com", "yahooligans.org", "acme.co.uk"] {
            assert!(!is_free_mail(d), "{d}");
        }
    }
}
