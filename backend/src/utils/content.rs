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

use once_cell::sync::Lazy;
use regex::Regex;

/// Wrap width for the plaintext rendering. 78 columns matches the
/// long-standing email convention (RFC 5322 + Markdown / Gmail / mutt
/// behaviour) so the result wraps cleanly in clients that don't
/// reflow on their own.
const PLAINTEXT_WRAP_COLS: usize = 78;

/// Convert HTML to a UTF-8 plaintext rendering.
///
/// Uses `html2text` for the block structure: paragraphs become
/// blank-line-separated, list items get a `•` or `1.` prefix, and blockquotes
/// get `>` prefixes, as mail clients quote. Inline formatting, heading levels
/// and code spans carry no marks, so a reader sees no stray `**`, `#` or
/// backticks. A link reads as its text with the address after it in
/// parentheses, or as the bare address when that is its text.
/// Overflowing the wrap width beats failing: deeply nested quotes or lists
/// would otherwise return `TooNarrow`. Reading from a byte slice cannot hit
/// an I/O error, so the empty fallback is unreachable in practice.
pub fn html_to_plaintext(html: &str) -> String {
    let (html, addresses) = inline_link_addresses(html);
    let mut text = html2text::config::with_decorator(MailDecorator)
        .allow_width_overflow()
        .string_from_read(html.as_bytes(), PLAINTEXT_WRAP_COLS)
        .unwrap_or_default();
    for (i, address) in addresses.iter().enumerate() {
        text = text.replacen(&address_token(i), address, 1);
    }
    text
}

/// An `<a>` element with an `href`, and what's inside it. The editor's
/// links don't nest, so the first `</a>` closes the one that opened.
static ANCHOR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?is)<a\b[^>]*?\bhref\s*=\s*"([^"]*)"[^>]*>(.*?)</a\s*>"#).expect("valid regex")
});

/// Stands in for a link address while the text wraps, so a long address is
/// never split across lines (which would stop it opening). Private-use
/// characters, which no comment contains.
fn address_token(i: usize) -> String {
    format!("\u{E000}{i}\u{E001}")
}

/// Write each link's address into the text after it, since plain text has
/// nowhere else to keep it, as a token for [`address_token`]. A link whose
/// text is already its address (or the address without `mailto:`) shows just
/// that.
fn inline_link_addresses(html: &str) -> (String, Vec<String>) {
    let mut addresses = Vec::new();
    let out = ANCHOR_RE.replace_all(html, |caps: &regex::Captures| {
        let inner = &caps[2];
        let href = html_escape::decode_html_entities(&caps[1]).into_owned();
        let text = html_escape::decode_html_entities(&HTML_TAG_RE.replace_all(inner, ""))
            .trim()
            .to_string();
        let shown = href.strip_prefix("mailto:").unwrap_or(&href).to_string();
        if href.is_empty() || href.starts_with('#') {
            return inner.to_string();
        }
        let token = address_token(addresses.len());
        let same = text == href || text == shown;
        addresses.push(shown);
        if same {
            token
        } else {
            format!("{inner} ({token})")
        }
    });
    (out.into_owned(), addresses)
}

/// `html2text`'s plain decorator without the Markdown-style marks.
#[derive(Clone, Debug)]
struct MailDecorator;

impl html2text::render::TextDecorator for MailDecorator {
    type Annotation = ();

    fn decorate_link_start(&mut self, _url: &str) -> (String, Self::Annotation) {
        (String::new(), ())
    }
    fn decorate_link_end(&mut self) -> String {
        String::new()
    }
    fn decorate_em_start(&self) -> (String, Self::Annotation) {
        (String::new(), ())
    }
    fn decorate_em_end(&self) -> String {
        String::new()
    }
    fn decorate_strong_start(&self) -> (String, Self::Annotation) {
        (String::new(), ())
    }
    fn decorate_strong_end(&self) -> String {
        String::new()
    }
    fn decorate_strikeout_start(&self) -> (String, Self::Annotation) {
        (String::new(), ())
    }
    fn decorate_strikeout_end(&self) -> String {
        String::new()
    }
    fn decorate_code_start(&self) -> (String, Self::Annotation) {
        (String::new(), ())
    }
    fn decorate_code_end(&self) -> String {
        String::new()
    }
    fn decorate_preformat_first(&self) -> Self::Annotation {}
    fn decorate_preformat_cont(&self) -> Self::Annotation {}
    fn decorate_image(&mut self, _src: &str, title: &str) -> (String, Self::Annotation) {
        (title.to_string(), ())
    }
    fn header_prefix(&self, _level: usize) -> String {
        String::new()
    }
    fn quote_prefix(&self) -> String {
        "> ".to_string()
    }
    fn unordered_item_prefix(&self) -> String {
        "• ".to_string()
    }
    fn ordered_item_prefix(&self, i: i64) -> String {
        format!("{i}. ")
    }
    fn make_subblock_decorator(&self) -> Self {
        self.clone()
    }
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

/// An opening or closing tag: `<b>`, `</p>`, `<br/>`, `<a href="...">`.
/// Text such as "a < b" or "<jane@example.com>" doesn't match.
static HTML_TAG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"</?[A-Za-z][A-Za-z0-9]*(\s[^<>]*)?/?>").expect("valid regex"));

/// The HTML for a reply template. One that carries tags is already HTML
/// and passes through as written; one without is plain text, so it is
/// escaped and keeps its line breaks. Variable tokens survive either way,
/// for the template renderer to fill in afterwards.
pub fn reply_template_html(template: &str) -> String {
    if HTML_TAG_RE.is_match(template) {
        template.to_string()
    } else {
        plaintext_to_html(template)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_reply_template_keeps_its_line_breaks_and_tokens() {
        let out = reply_template_html("Hi {{customer_name}},\n\nThanks for writing in.");
        assert_eq!(
            out,
            "Hi {{customer_name}},<br>\n<br>\nThanks for writing in."
        );
    }

    #[test]
    fn a_reply_template_with_tags_passes_through() {
        let template = "Hi {{ customer_name }},<br><b>Thanks</b> for writing in.";
        assert_eq!(reply_template_html(template), template);
    }

    #[test]
    fn angle_brackets_that_are_not_tags_are_escaped() {
        let out = reply_template_html("If a < b, write to <help@example.com>.");
        assert!(out.contains("a &lt; b"), "got: {out}");
        assert!(out.contains("&lt;help@example.com&gt;"), "got: {out}");
    }

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
    fn html_to_plaintext_leaves_no_markdown_marks() {
        let out = html_to_plaintext(
            "<h2>A heading</h2><p><strong>bold words</strong>, <em>italic words</em> and \
             <code>code</code></p><ul><li><p>apple item</p></li><li><p>banana item</p></li></ul>\
             <ol><li><p>first</p></li></ol>",
        );
        for mark in ["**", "*", "#", "`", "[", "]"] {
            assert!(!out.contains(mark), "{mark:?} in {out:?}");
        }
        assert!(out.contains("A heading"), "{out:?}");
        assert!(out.contains("bold words, italic words and code"), "{out:?}");
        assert!(out.contains("• apple item"), "{out:?}");
        assert!(out.contains("• banana item"), "{out:?}");
        assert!(out.contains("1. first"), "{out:?}");
    }

    #[test]
    fn html_to_plaintext_writes_a_link_address_after_its_text() {
        let out = html_to_plaintext(
            "<p>See the <a href=\"https://nosdesk.com/?a=1&amp;b=2\" target=\"_blank\">\
             <strong>Nosdesk site</strong></a>, <a href=\"https://nosdesk.dev/docs\">\
             https://nosdesk.dev/docs</a> or <a href=\"mailto:help@example.com\">\
             help@example.com</a>.</p>",
        );
        assert!(
            out.contains("Nosdesk site (https://nosdesk.com/?a=1&b=2)"),
            "{out:?}"
        );
        assert!(out.contains("https://nosdesk.dev/docs"), "{out:?}");
        assert!(!out.contains("docs ("), "{out:?}");
        assert!(out.contains("help@example.com."), "{out:?}");
        assert!(!out.contains("mailto"), "{out:?}");

        // An address longer than a line stays whole, so it still opens.
        let long = format!("https://example.com/{}", "a".repeat(100));
        let out = html_to_plaintext(&format!("<p>Read <a href=\"{long}\">this</a> first.</p>"));
        assert!(out.contains(&long), "{out:?}");
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
