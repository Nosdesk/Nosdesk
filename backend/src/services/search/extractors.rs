//! Text extraction utilities for Yjs documents and HTML content

use once_cell::sync::Lazy;
use regex::Regex;
use std::panic;
use tracing::debug;
use yrs::{Doc, Transact, ReadTxn, WriteTxn, GetString, Options, updates::decoder::Decode, Update, XmlFragment, XmlOut};

// Pre-compiled regexes for performance
static HTML_TAG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"<[^>]+>").unwrap()
});

static WHITESPACE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\s+").unwrap()
});

static MENTION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"@\[[^\]]+\]\([a-f0-9-]+\)").unwrap()
});

/// Strip HTML tags from content
pub fn strip_html(content: &str) -> String {
    let without_html = HTML_TAG_RE.replace_all(content, " ");
    let normalized = WHITESPACE_RE.replace_all(&without_html, " ");
    normalized.trim().to_string()
}

/// Strip HTML and clean up mentions for plain text extraction
pub fn strip_html_and_mentions(content: &str) -> String {
    // Remove @[Name](uuid) mentions entirely for indexing
    let without_mentions = MENTION_RE.replace_all(content, "");
    strip_html(&without_mentions)
}

/// Recursively extract plain text from an XmlOut node
fn extract_text_from_xml_node(node: &XmlOut, txn: &yrs::Transaction) -> String {
    match node {
        XmlOut::Text(text_ref) => {
            // XmlTextRef::get_string returns the text content
            // Use panic::catch_unwind because Yjs can panic on corrupted data
            match panic::catch_unwind(panic::AssertUnwindSafe(|| {
                text_ref.get_string(txn)
            })) {
                Ok(s) => s,
                Err(_) => String::new(),
            }
        }
        XmlOut::Element(elem_ref) => {
            // Recursively extract text from element's children
            let mut text = String::new();
            for child in elem_ref.children(txn) {
                let child_text = extract_text_from_xml_node(&child, txn);
                if !child_text.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&child_text);
                }
            }
            text
        }
        XmlOut::Fragment(frag_ref) => {
            // Recursively extract text from fragment's children
            let mut text = String::new();
            for child in frag_ref.children(txn) {
                let child_text = extract_text_from_xml_node(&child, txn);
                if !child_text.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&child_text);
                }
            }
            text
        }
    }
}

/// Extract plain text from a Yjs document binary blob
///
/// Yjs documents in Nosdesk use ProseMirror-style content.
/// This function decodes the Yjs updates and extracts text content.
pub fn extract_text_from_yjs(yjs_data: &[u8]) -> Option<String> {
    if yjs_data.is_empty() {
        return None;
    }

    // Create a new Yjs document with GC disabled for reading
    let options = Options {
        skip_gc: true,
        ..Default::default()
    };
    let doc = Doc::with_options(options);

    // Initialize the prosemirror XmlFragment BEFORE applying update
    // This is critical - the fragment must exist before the update is applied
    {
        let mut txn = doc.transact_mut();
        let _ = txn.get_or_insert_xml_fragment("prosemirror");
    }

    // Decode and apply the update
    let update = match Update::decode_v1(yjs_data) {
        Ok(u) => u,
        Err(_) => return None,
    };

    {
        let mut txn = doc.transact_mut();
        if txn.apply_update(update).is_err() {
            return None;
        }
    }

    // Extract text content from the prosemirror fragment by traversing children
    let txn = doc.transact();
    if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
        let mut text_parts = Vec::new();

        // Iterate through top-level children (paragraphs, headings, etc.)
        for child in fragment.children(&txn) {
            let child_text = extract_text_from_xml_node(&child, &txn);
            if !child_text.is_empty() {
                text_parts.push(child_text);
            }
        }

        if text_parts.is_empty() {
            return None;
        }

        let joined = text_parts.join(" ");
        // Strip any remaining XML/HTML tags (e.g., <strong>, <em>, etc.)
        let clean_text = HTML_TAG_RE.replace_all(&joined, "").to_string();
        // Normalize whitespace
        let normalized = WHITESPACE_RE.replace_all(&clean_text, " ").trim().to_string();

        if normalized.is_empty() {
            None
        } else {
            debug!(len = normalized.len(), "Extracted text from Yjs document");
            Some(normalized)
        }
    } else {
        None
    }
}

/// Create a preview snippet from content (truncated with ellipsis)
pub fn create_preview(content: &str, max_len: usize) -> String {
    let cleaned = strip_html_and_mentions(content);
    if cleaned.len() <= max_len {
        cleaned
    } else {
        // Find a word boundary near the max length
        let truncated: String = cleaned.chars().take(max_len).collect();
        if let Some(last_space) = truncated.rfind(' ') {
            format!("{}...", &truncated[..last_space])
        } else {
            format!("{}...", truncated)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html() {
        assert_eq!(
            strip_html("<p>Hello <strong>world</strong>!</p>"),
            "Hello world !"
        );
        assert_eq!(
            strip_html("No HTML here"),
            "No HTML here"
        );
        assert_eq!(
            strip_html("<div>Multiple\n\n\nspaces</div>"),
            "Multiple spaces"
        );
    }

    #[test]
    fn test_create_preview() {
        let content = "This is a long piece of content that needs to be truncated for preview purposes.";
        let preview = create_preview(content, 30);
        assert!(preview.len() <= 33); // 30 + "..."
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn test_strip_html_and_mentions() {
        let content = "<p>Hello @[John Doe](abc-123), how are you?</p>";
        let result = strip_html_and_mentions(content);
        assert_eq!(result, "Hello , how are you?");
    }
}
