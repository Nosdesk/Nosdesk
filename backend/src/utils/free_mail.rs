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
    "zoho.com",
];

/// Whether `address` (or a bare domain) is at a free-mail provider.
pub fn is_free_mail(address: &str) -> bool {
    let domain = address
        .rsplit('@')
        .next()
        .unwrap_or(address)
        .trim()
        .to_lowercase();
    FREE_MAIL_DOMAINS.contains(&domain.as_str())
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
}
