//! Native-first render-tier classification for inbound email comments.
//!
//! The heavy iframe + sanitiser stack exists for the worst case
//! (marketing HTML, Outlook Word-soup). Applying it to every message,
//! including a one-line plaintext "thanks!", is what makes the ticket
//! thread look boxed-off and mounts an iframe per comment. Inbound mail
//! is really three tiers, and only one needs an iframe:
//!
//!   - `text`   — plaintext / format=flowed. Rendered as a native
//!     bubble (escape + linkify + pre-wrap) by the frontend.
//!   - `simple` — human HTML (Gmail / Apple / Outlook personal
//!     replies): inline marks only. Reduced here to a semantic-HTML
//!     subset (no `style`/`class`/`id`, no images, scheme-checked
//!     links) and rendered inline by the frontend with no iframe.
//!   - `rich`   — newsletters, layout tables, `<style>` blocks,
//!     image-heavy or large bodies. Kept as the full sanitised HTML
//!     for the sandboxed iframe.
//!
//! Classification runs at ingest, on already-sanitised content (so the
//! `simple` reduction can be injected directly), and the chosen
//! `render_kind` is persisted on the comment row. The split quoted
//! history is reduced the same way so it never needs an iframe either.
//!
//! Safety: the `simple` subset is stripped to a semantic tag/attribute
//! allowlist with no scripts, no `style`/`class`/`id` (which kills the
//! DOM-clobbering surface — Fastmail's lesson), and no images or remote
//! resources. That is the same risk profile as the markdown-generated
//! HTML the app already renders for agent comments, so it is safe to
//! render inline without an iframe boundary.
//!
//! Inline images live on the `rich` (iframe) path, as file attachments,
//! or behind "View original"; the `simple` tier intentionally drops
//! them (usually signatures / tracking pixels), and an image-heavy body
//! is classified `rich` so genuine screenshots still render.

use std::collections::HashSet;

use once_cell::sync::Lazy;

use crate::models::ContentFormat;

/// Render tier for one comment. Serialised to the `render_kind`
/// VARCHAR(16) column via [`RenderKind::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderKind {
    Text,
    Simple,
    Rich,
}

impl RenderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RenderKind::Text => "text",
            RenderKind::Simple => "simple",
            RenderKind::Rich => "rich",
        }
    }
}

/// Outcome of classifying one inbound body: the chosen tier plus the
/// content to store. For `simple` the content is the reduced subset;
/// for `text`/`rich` it is the input unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub kind: RenderKind,
    pub new_content: String,
    pub quoted_content: Option<String>,
}

/// HTML bodies larger than this (post-sanitise) are documents, not
/// replies — classify `rich` regardless of structure.
const RICH_BYTE_THRESHOLD: usize = 24 * 1024;

/// More than this many `<img>` tags reads as a marketing layout rather
/// than a personal reply with a signature logo.
const RICH_IMG_THRESHOLD: usize = 3;

/// Classify an already-sanitised inbound body and return the content to
/// persist. `new_content` / `quoted_content` are the quote-split parts.
pub fn classify(
    format: ContentFormat,
    new_content: &str,
    quoted_content: Option<&str>,
) -> Classified {
    // Only HTML email can be `simple`/`rich`; plaintext (and the
    // never-produced-inbound Markdown variant) is always a text bubble.
    if format != ContentFormat::Html {
        return Classified {
            kind: RenderKind::Text,
            new_content: new_content.to_string(),
            quoted_content: quoted_content.map(str::to_string),
        };
    }

    if is_rich(new_content) {
        Classified {
            kind: RenderKind::Rich,
            new_content: new_content.to_string(),
            quoted_content: quoted_content.map(str::to_string),
        }
    } else {
        Classified {
            kind: RenderKind::Simple,
            new_content: reduce_to_subset(new_content),
            quoted_content: quoted_content.map(reduce_to_subset),
        }
    }
}

/// True when reducing the (sanitised) HTML to the semantic subset would
/// lose something material — i.e. it carries document/layout structure
/// the subset can't represent. Conservative: a false "rich" only costs
/// an iframe; a false "simple" would silently drop content.
fn is_rich(html: &str) -> bool {
    if html.len() > RICH_BYTE_THRESHOLD {
        return true;
    }
    let lower = html.to_ascii_lowercase();

    // Layout / document constructs that flattening to inline marks
    // would destroy.
    const HARD_SIGNALS: &[&str] = &[
        "<table", // layout tables (newsletters)
        "<style", // document-level CSS
        "<center",
        "<font", // legacy layout
        "background:",
        "background-image",
        "background=", // newsletter canvases
        "position:",   // absolute/fixed positioning
    ];
    if HARD_SIGNALS.iter().any(|s| lower.contains(s)) {
        return true;
    }

    // Image-heavy => marketing layout, not a personal reply. A lone
    // signature logo stays `simple` (and is dropped by the reducer).
    if lower.matches("<img").count() > RICH_IMG_THRESHOLD {
        return true;
    }

    false
}

/// Tags kept in the `simple` subset: inline marks, links, lists,
/// blockquote, code, and neutral structural wrappers. Everything else
/// (including `img`, `table`, `style`, `font`) is dropped, along with
/// all `style`/`class`/`id` attributes.
static SUBSET_TAGS: &[&str] = &[
    "p",
    "br",
    "div",
    "span",
    "strong",
    "b",
    "em",
    "i",
    "u",
    "s",
    "a",
    "ul",
    "ol",
    "li",
    "blockquote",
    "code",
    "pre",
    "hr",
    "h1",
    "h2",
    "h3",
];

static SUBSET: Lazy<ammonia::Builder<'static>> = Lazy::new(|| {
    let mut b = ammonia::Builder::default();
    // Replace ammonia's broad default tag set with our tight subset.
    b.tags(SUBSET_TAGS.iter().copied().collect::<HashSet<_>>());
    // Links only carry href; no style/class/id on anything. ammonia's
    // defaults already add rel="noopener noreferrer" and force target
    // handling, and strip attributes not in the per-tag allowlist.
    b.tag_attributes(
        [("a", ["href"].iter().copied().collect::<HashSet<_>>())]
            .into_iter()
            .collect(),
    );
    b.url_schemes(
        ["http", "https", "mailto", "tel", "sms"]
            .iter()
            .copied()
            .collect(),
    );
    b
});

/// Reduce sanitised HTML to the semantic subset. Idempotent.
fn reduce_to_subset(html: &str) -> String {
    SUBSET.clean(html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_is_text() {
        let c = classify(ContentFormat::Plaintext, "hi there\nthanks", None);
        assert_eq!(c.kind, RenderKind::Text);
        assert_eq!(c.new_content, "hi there\nthanks");
    }

    #[test]
    fn human_gmail_reply_is_simple_and_reduced() {
        // A typical Gmail reply: div wrapper + inline marks + a link.
        let html = r#"<div dir="ltr"><p>Thanks, that <b>fixed</b> it!</p>
            <p>Can I add a <a href="https://x.test/seat" style="color:#06c">2nd seat</a>?</p></div>"#;
        let c = classify(ContentFormat::Html, html, None);
        assert_eq!(c.kind, RenderKind::Simple);
        // style attribute dropped, marks + link preserved.
        assert!(c.new_content.contains("<b>fixed</b>") || c.new_content.contains("<strong>fixed"));
        assert!(c.new_content.contains("2nd seat"));
        assert!(!c.new_content.contains("style="), "style must be stripped");
        assert!(!c.new_content.contains("color:#06c"));
    }

    #[test]
    fn layout_table_is_rich_and_unchanged() {
        let html = r#"<table width="600"><tr><td>Newsletter</td></tr></table>"#;
        let c = classify(ContentFormat::Html, html, None);
        assert_eq!(c.kind, RenderKind::Rich);
        assert_eq!(
            c.new_content, html,
            "rich content is kept verbatim for the iframe"
        );
    }

    #[test]
    fn style_block_is_rich() {
        let html = r#"<style>.x{color:red}</style><p>hello</p>"#;
        assert_eq!(
            classify(ContentFormat::Html, html, None).kind,
            RenderKind::Rich
        );
    }

    #[test]
    fn image_heavy_is_rich() {
        let html = format!("<p>hi</p>{}", "<img src=\"https://x/y.png\">".repeat(4));
        assert_eq!(
            classify(ContentFormat::Html, &html, None).kind,
            RenderKind::Rich
        );
    }

    #[test]
    fn single_signature_image_stays_simple_image_dropped() {
        let html = r#"<p>Cheers,</p><p>Jo</p><img src="https://x/logo.png">"#;
        let c = classify(ContentFormat::Html, html, None);
        assert_eq!(c.kind, RenderKind::Simple);
        assert!(!c.new_content.contains("<img"), "subset drops images");
        assert!(c.new_content.contains("Cheers"));
    }

    #[test]
    fn oversized_body_is_rich() {
        let html = format!("<p>{}</p>", "x".repeat(RICH_BYTE_THRESHOLD + 1));
        assert_eq!(
            classify(ContentFormat::Html, &html, None).kind,
            RenderKind::Rich
        );
    }

    #[test]
    fn quoted_content_is_reduced_for_simple() {
        let new = "<p>reply</p>";
        let quoted =
            r#"<blockquote><p>earlier <span style="color:red">note</span></p></blockquote>"#;
        let c = classify(ContentFormat::Html, new, Some(quoted));
        assert_eq!(c.kind, RenderKind::Simple);
        let q = c.quoted_content.expect("quoted preserved");
        assert!(q.contains("earlier"));
        assert!(!q.contains("style="), "quoted is reduced too");
    }

    #[test]
    fn subset_strips_scripts_and_handlers() {
        let html = r#"<p onclick="evil()">hi</p><script>alert(1)</script>"#;
        let c = classify(ContentFormat::Html, html, None);
        assert_eq!(c.kind, RenderKind::Simple);
        assert!(!c.new_content.contains("onclick"));
        assert!(!c.new_content.contains("<script"));
        assert!(!c.new_content.contains("alert(1)"));
    }
}
