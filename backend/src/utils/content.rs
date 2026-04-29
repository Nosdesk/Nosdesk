//! Content-format interconversion shared by every transport that has
//! to emit a comment in a representation other than the one it was
//! stored in.
//!
//! Today's only consumer is the email outbound relay, which needs a
//! plaintext counterpart of the editor's HTML so the
//! `multipart/alternative` message has a fallback for clients that
//! prefer plaintext or don't render HTML at all. Future channels (a
//! webhook that posts plain text, an SMS bridge, a Slack adapter) reuse
//! the same helpers — the conversions belong at the channel boundary,
//! not duplicated in each adapter.

/// Wrap width for the plaintext rendering. 78 columns matches the
/// long-standing email convention (RFC 5322 + Markdown / Gmail / mutt
/// behaviour) so the result wraps cleanly in clients that don't
/// reflow on their own.
const PLAINTEXT_WRAP_COLS: usize = 78;

/// Convert HTML to a UTF-8 plaintext rendering.
///
/// Uses `html2text`, which preserves block structure (paragraphs become
/// blank-line-separated, lists get `*` / `1.` prefixes, blockquotes get
/// `>` prefixes — exactly what an email recipient expects). Inline
/// formatting (`<strong>`, `<em>`) is dropped because plaintext can't
/// represent it without ASCII-art crutches that look worse than no
/// formatting at all.
pub fn html_to_plaintext(html: &str) -> String {
    html2text::from_read(html.as_bytes(), PLAINTEXT_WRAP_COLS)
}

/// Wrap a plaintext string in the minimal HTML needed for it to render
/// faithfully inside an HTML email body. Escapes the special characters
/// and turns hard newlines into `<br>` so a recipient's HTML view
/// matches their plaintext view line-for-line.
///
/// Used when we need to embed a plaintext comment (an inbound email
/// stored as plaintext, a future SMS reply) inside the HTML half of a
/// `multipart/alternative` message.
pub fn plaintext_to_html(text: &str) -> String {
    // `html-escape` handles `<`, `>`, `&`, `"`, `'`. Newlines aren't
    // special in HTML escaping but are visually load-bearing, so we
    // convert them to `<br>` after escaping.
    html_escape::encode_safe(text).replace('\n', "<br>\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_plaintext_renders_paragraphs_as_blank_separated() {
        let out = html_to_plaintext("<p>hello</p><p>world</p>");
        // Paragraph break shows up as a blank line — what an email
        // recipient sees in a plaintext-only client.
        assert!(out.contains("hello"));
        assert!(out.contains("world"));
        assert!(out.contains("\n\n"));
    }

    #[test]
    fn html_to_plaintext_strips_inline_formatting() {
        let out = html_to_plaintext("<p>This is <strong>bold</strong> and <em>italic</em>.</p>");
        // No `<strong>` / `<em>` markers leak through.
        assert!(!out.contains('<'));
        assert!(out.contains("bold"));
        assert!(out.contains("italic"));
    }

    #[test]
    fn html_to_plaintext_renders_blockquote_with_arrow_prefix() {
        // `<blockquote>` is how the quoted-reply prelude is built;
        // html2text must emit `>` line prefixes so the plaintext part
        // matches what mail clients render for quoted reply chains.
        let out = html_to_plaintext("<blockquote><p>quoted line</p></blockquote>");
        assert!(out.contains("> "), "expected `> ` prefix, got: {out:?}");
    }

    #[test]
    fn plaintext_to_html_escapes_special_chars() {
        let out = plaintext_to_html("a < b && b > c");
        assert!(out.contains("&lt;"));
        assert!(out.contains("&gt;"));
        assert!(out.contains("&amp;"));
    }

    #[test]
    fn plaintext_to_html_converts_newlines_to_br() {
        let out = plaintext_to_html("line one\nline two");
        assert!(out.contains("<br>"));
        // The literal newline is preserved alongside the <br> so the
        // resulting HTML source remains readable when inspected.
        assert!(out.contains("line one"));
        assert!(out.contains("line two"));
    }
}
