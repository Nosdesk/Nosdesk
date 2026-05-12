//! Two-form representation of an outbound reply.
//!
//! The email channel needs both an HTML body and a plaintext body so
//! it can ship a `multipart/alternative` message. Other transports
//! pick one — chat that wants Markdown reads `text`; an HTML-only
//! webhook reads `html`. Holding both in a single value lets the
//! signature and quoted-reply helpers stay format-agnostic: each
//! transformation operates on the bundle and updates whichever forms
//! it has something to contribute to.
//!
//! The struct is built with method chaining so the orchestration step
//! in `outbound.rs` reads as a top-down recipe:
//!
//! ```ignore
//! let body = ReplyBody::from_comment(comment)
//!     .append_signature(conn, comment.user_uuid)
//!     .prepend_quote(conn, channel, ticket);
//! ```

use crate::models::{Comment, ContentFormat};
use crate::utils::content::{html_to_plaintext, plaintext_to_html};

/// Reply contents in both representations a transport might want.
///
/// Invariant: `html` and `text` describe the same logical message.
/// Helpers that mutate one always mutate the other in lockstep.
#[derive(Debug, Clone)]
pub struct ReplyBody {
    pub html: String,
    pub text: String,
}

impl ReplyBody {
    /// Seed both forms from a stored comment. Looks at the comment's
    /// `content_format` so an inbound email (stored as plaintext) and
    /// a tech-authored comment (stored as HTML) both end up with a
    /// faithful pair of representations.
    pub fn from_comment(comment: &Comment) -> Self {
        match comment.content_format {
            ContentFormat::Html => Self {
                html: comment.content.clone(),
                text: html_to_plaintext(&comment.content),
            },
            ContentFormat::Plaintext => Self {
                text: comment.content.clone(),
                html: plaintext_to_html(&comment.content),
            },
            ContentFormat::Markdown => {
                // Markdown isn't produced by any path we ship today.
                // Treat its bytes as plaintext until a real consumer
                // (Slack inbound, scripted CLI sender) lands and it's
                // worth pulling in a Markdown renderer.
                Self {
                    text: comment.content.clone(),
                    html: plaintext_to_html(&comment.content),
                }
            }
        }
    }

    /// Append text to both representations. `html_fragment` is added
    /// verbatim — callers are responsible for escaping any
    /// caller-controlled text inside it.
    pub fn append(mut self, html_fragment: &str, text_fragment: &str) -> Self {
        self.html.push_str(html_fragment);
        self.text.push_str(text_fragment);
        self
    }

    /// Symmetric prepend. Used by the quoted-reply helper, which
    /// inserts the prior message *after* the new reply (the order on
    /// the wire is: tech's reply, signature, quoted prior message).
    pub fn append_block(self, html_fragment: &str, text_fragment: &str) -> Self {
        self.append(html_fragment, text_fragment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn comment_with(content: &str, format: ContentFormat) -> Comment {
        Comment {
            id: 1,
            content: content.into(),
            ticket_id: 1,
            user_uuid: Uuid::nil(),
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            is_edited: false,
            edit_count: 0,
            channel_metadata: None,
            is_internal: false,
            deleted_at: None,
            content_format: format,
            body_text: None,
            body_html: None,
            new_content: None,
            quoted_content: None,
            raw_source_uri: None,
            sanitised_html: None,
        }
    }

    #[test]
    fn from_html_comment_derives_plaintext() {
        let body = ReplyBody::from_comment(&comment_with("<p>hello</p>", ContentFormat::Html));
        assert!(body.html.contains("<p>hello</p>"));
        assert!(body.text.contains("hello"));
        assert!(!body.text.contains('<'));
    }

    #[test]
    fn from_plaintext_comment_escapes_into_html() {
        let body = ReplyBody::from_comment(&comment_with(
            "two < three\nand more",
            ContentFormat::Plaintext,
        ));
        assert_eq!(body.text, "two < three\nand more");
        assert!(body.html.contains("&lt;"));
        assert!(body.html.contains("<br>"));
    }

    #[test]
    fn append_grows_both_forms() {
        let body = ReplyBody {
            html: "<p>hi</p>".into(),
            text: "hi".into(),
        }
        .append("<br>--", "\n--");
        assert_eq!(body.html, "<p>hi</p><br>--");
        assert_eq!(body.text, "hi\n--");
    }
}
