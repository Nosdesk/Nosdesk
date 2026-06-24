//! LDAP escaping. Two DISTINCT contexts — applying the wrong one leaves an
//! injection hole, so they are separate functions:
//! - [`escape_filter_value`] for assertion values inside an RFC 4515 search
//!   filter, e.g. the username substituted into `(uid={username})`.
//! - [`escape_dn_value`] for an attribute value inside an RFC 4514 DN.
//!
//! Filters and DNs must NEVER be built by string concatenation of un-escaped
//! input.

/// Escape a value for use inside an RFC 4515 search-filter assertion. Escapes
/// the filter metacharacters `\ * ( )` and NUL as `\<hex>`; all other
/// characters (including UTF-8) pass through, which servers accept.
pub fn escape_filter_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\5c"),
            '*' => out.push_str("\\2a"),
            '(' => out.push_str("\\28"),
            ')' => out.push_str("\\29"),
            '\0' => out.push_str("\\00"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape an attribute value for use inside an RFC 4514 distinguished name.
/// Escapes `" + , ; < > \` everywhere, NUL as `\00`, a leading `#`, and a
/// leading or trailing space.
pub fn escape_dn_value(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let last = chars.len().saturating_sub(1);
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '"' | '+' | ',' | ';' | '<' | '>' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\0' => out.push_str("\\00"),
            ' ' if i == 0 || i == last => {
                out.push('\\');
                out.push(' ');
            }
            '#' if i == 0 => {
                out.push('\\');
                out.push('#');
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_escapes_metacharacters() {
        assert_eq!(escape_filter_value("plain"), "plain");
        assert_eq!(escape_filter_value("a*b"), "a\\2ab");
        assert_eq!(escape_filter_value("a(b)c"), "a\\28b\\29c");
        assert_eq!(escape_filter_value("a\\b"), "a\\5cb");
        // The classic injection: username "*)(uid=*" must not break out.
        assert_eq!(escape_filter_value("*)(uid=*"), "\\2a\\29\\28uid=\\2a");
        // UTF-8 passes through.
        assert_eq!(escape_filter_value("José"), "José");
    }

    #[test]
    fn dn_escapes_special_chars_and_positions() {
        assert_eq!(escape_dn_value("plain"), "plain");
        assert_eq!(escape_dn_value("a,b"), "a\\,b");
        assert_eq!(escape_dn_value("a+b;c"), "a\\+b\\;c");
        assert_eq!(escape_dn_value("a\"b"), "a\\\"b");
        // Leading/trailing space + leading hash are positional.
        assert_eq!(escape_dn_value(" x "), "\\ x\\ ");
        assert_eq!(escape_dn_value("#x"), "\\#x");
        // A hash that is not leading is left alone.
        assert_eq!(escape_dn_value("a#b"), "a#b");
        // A DN-injection attempt is neutralized.
        assert_eq!(escape_dn_value("admin,ou=evil"), "admin\\,ou=evil");
    }
}
