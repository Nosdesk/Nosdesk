//! Unicode-safe truncation for user-controlled strings.

/// Prefix of `s` with at most `max_chars` Unicode scalar values (never panics on boundaries).
#[inline]
pub fn char_prefix(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Prefix bounded by UTF-8 byte length `max_bytes`, always split on a character boundary.
#[inline]
pub fn byte_prefix(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let end = s.floor_char_boundary(max_bytes);
    s[..end].to_string()
}

/// Like [`byte_prefix`], but when truncated appends "`... [truncated]`".
pub fn byte_prefix_with_ellipsis(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let end = s.floor_char_boundary(max_bytes);
    format!("{}... [truncated]", &s[..end])
}

/// Replace CRLF so single-line tracing fields resist log-structure injection from user input.
#[inline]
pub fn strip_line_breaks_for_log_field(s: &str) -> String {
    s.chars().map(|c| if matches!(c, '\n' | '\r') { ' ' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_prefix_truncates_smiley_beyond_four_chars_without_panic() {
        let s = "😀😀😀😀😀"; // 20 UTF-8 bytes, 5 chars
        assert_eq!(char_prefix(s, 4), "😀😀😀😀");
    }

    #[test]
    fn byte_prefix_respects_boundary() {
        let s = "a😀"; // letter + 4-byte emoji
        assert_eq!(byte_prefix(s, 2), "a"); // truncate before emoji
        assert_eq!(byte_prefix_with_ellipsis(s, 3), "a... [truncated]");
    }
}
