//! Mention extraction and preview text for comment notifications.
//!
//! Two composers write mentions in two shapes. The ProseMirror editor behind
//! ticket comments stores an HTML span carrying `data-mention="true"` and
//! `data-uuid`; the plain-text `MentionInput` behind canned-response templates
//! writes `@[Name](uuid)`. Both are recognised here, and nothing else in the
//! backend parses mention syntax.

use once_cell::sync::Lazy;
use regex::Regex;
use uuid::Uuid;

const UUID: &str = r"[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}";

static MARKDOWN_UUID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(&format!(r"@\[[^\]]+\]\(({UUID})\)")).unwrap());
static MARKDOWN_DISPLAY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"@\[([^\]]+)\]\([a-f0-9-]+\)").unwrap());
/// An opening tag that declares itself a mention. Attribute order is the
/// editor's business, so the uuid and name are pulled from the matched tag
/// separately rather than positionally.
static SPAN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"<[a-zA-Z]+\b[^>]*\bdata-mention="true"[^>]*>"#).unwrap());
/// The whole mention element, for the preview: the span's own text is the
/// display name the editor rendered, and the attribute is the fallback.
static SPAN_ELEMENT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?s)<span\b[^>]*\bdata-mention="true"[^>]*>(.*?)</span>"#).unwrap());
static SPAN_UUID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(&format!(r#"\bdata-uuid="({UUID})""#)).unwrap());
static SPAN_NAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\bdata-name="([^"]*)""#).unwrap());
static HTML_TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
static WHITESPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

/// Every user mentioned in `content`, in either syntax, deduplicated and
/// sorted.
pub fn parse_mentions(content: &str) -> Vec<Uuid> {
    let markdown = MARKDOWN_UUID_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).and_then(|m| Uuid::parse_str(m.as_str()).ok()));
    let spans = SPAN_RE.find_iter(content).filter_map(|tag| {
        SPAN_UUID_RE
            .captures(tag.as_str())
            .and_then(|cap| cap.get(1))
            .and_then(|m| Uuid::parse_str(m.as_str()).ok())
    });
    let mut mentions: Vec<Uuid> = markdown.chain(spans).collect();
    mentions.sort();
    mentions.dedup();
    mentions
}

/// Plain text for a notification body: mentions reduced to `@Name`, tags
/// stripped, whitespace collapsed.
pub fn strip_html_for_preview(content: &str) -> String {
    let markdown = MARKDOWN_DISPLAY_RE.replace_all(content, "@$1");
    let spans = SPAN_ELEMENT_RE.replace_all(&markdown, |caps: &regex::Captures| {
        let inner = HTML_TAG_RE.replace_all(&caps[1], "");
        let inner = inner.trim();
        if !inner.is_empty() {
            return format!("@{}", inner.trim_start_matches('@'));
        }
        SPAN_NAME_RE
            .captures(&caps[0])
            .map(|c| format!("@{}", &c[1]))
            .unwrap_or_default()
    });
    let without_html = HTML_TAG_RE.replace_all(&spans, "");
    let normalized = WHITESPACE_RE.replace_all(&without_html, " ");
    normalized.trim().to_string()
}

/// Truncate to `max_len` characters, marking the cut.
pub fn truncate_preview(text: &str, max_len: usize) -> String {
    if text.chars().count() > max_len {
        format!("{}...", text.chars().take(max_len).collect::<String>())
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0192aaaa-0000-7000-8000-000000000001";
    const B: &str = "0192aaaa-0000-7000-8000-000000000002";

    #[test]
    fn markdown_form() {
        let got = parse_mentions(&format!("hi @[Alex Example]({A}) and @[Bo]({B})"));
        assert_eq!(
            got,
            vec![Uuid::parse_str(A).unwrap(), Uuid::parse_str(B).unwrap()]
        );
    }

    #[test]
    fn span_form_as_the_composer_stores_it() {
        let html = format!(
            r#"<p><span data-mention="true" data-uuid="{A}" data-name="Alex Example" data-avatar-url="" class="mention-chip" contenteditable="false">@Alex Example</span> can you look?</p>"#
        );
        assert_eq!(parse_mentions(&html), vec![Uuid::parse_str(A).unwrap()]);
    }

    #[test]
    fn span_form_attribute_order_does_not_matter() {
        let html =
            format!(r#"<span class="mention-chip" data-uuid="{A}" data-mention="true">x</span>"#);
        assert_eq!(parse_mentions(&html), vec![Uuid::parse_str(A).unwrap()]);
    }

    #[test]
    fn span_without_mention_flag_is_not_a_mention() {
        let html = format!(r#"<span data-uuid="{A}">not a mention</span>"#);
        assert!(parse_mentions(&html).is_empty());
    }

    #[test]
    fn both_forms_dedupe() {
        let mixed =
            format!(r#"@[Alex]({A}) <span data-mention="true" data-uuid="{A}">@Alex</span>"#);
        assert_eq!(parse_mentions(&mixed), vec![Uuid::parse_str(A).unwrap()]);
    }

    #[test]
    fn preview_reduces_both_forms_to_at_name() {
        let html = format!(
            r#"<p>ping <span data-mention="true" data-uuid="{A}" data-name="Alex">@Alex</span>  and @[Bo]({B})</p>"#
        );
        assert_eq!(strip_html_for_preview(&html), "ping @Alex and @Bo");
    }

    #[test]
    fn preview_uses_name_attribute_when_span_is_empty() {
        let html =
            format!(r#"<span data-mention="true" data-uuid="{A}" data-name="Alex"></span> hi"#);
        assert_eq!(strip_html_for_preview(&html), "@Alex hi");
    }

    #[test]
    fn truncate_counts_characters_not_bytes() {
        assert_eq!(truncate_preview("héllo wörld", 5), "héllo...");
        assert_eq!(truncate_preview("short", 10), "short");
    }
}
