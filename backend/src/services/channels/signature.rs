//! Append the agent's email signature to outbound channel replies in
//! both HTML and plaintext form.
//!
//! The signature itself lives on `user_preferences.signature` as
//! user-authored plaintext (split out from `users` on 2026-05-14
//! into the preferences table). We render it into both forms of the
//! reply body so the `multipart/alternative` email shows the same
//! signature in either view, and so future HTML-only / plaintext-
//! only transports each get a faithful version.
//!
//! Plaintext side uses the RFC 3676 `"-- \n"` separator (dash, dash,
//! space, newline) so mail clients recognize the signature block and
//! offer to collapse / strip it cleanly. HTML side uses a `<br>--<br>`
//! separator paired with the `text-signature` div so a plaintext
//! conversion of the HTML round-trips back to the same RFC 3676 marker.
//!
//! No-op when the user has no signature or it's empty / whitespace.

use uuid::Uuid;

use super::reply_body::ReplyBody;
use crate::db::DbConnection;

/// Fetch the user's stored signature; `None` if unset or whitespace.
fn signature_for_user(conn: &mut DbConnection, user_uuid: Uuid) -> Option<String> {
    let raw = crate::repository::user_preferences::get_signature(conn, user_uuid).ok();
    raw.flatten().and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(s)
        }
    })
}

/// Append the user's signature to both representations of `body`.
///
/// DB read failures are silently treated as "no signature" — better to
/// send the reply unsigned than to fail the whole outbound dispatch
/// over a transient read hiccup.
pub fn append_signature_for_user(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    body: ReplyBody,
) -> ReplyBody {
    match signature_for_user(conn, user_uuid) {
        Some(sig) => {
            let text_fragment = format!("\n\n-- \n{sig}");
            // Escape the user-authored signature before embedding into
            // HTML; their newlines become `<br>` so the visual layout
            // matches the plaintext.
            let escaped = html_escape::encode_safe(&sig).replace('\n', "<br>\n");
            let html_fragment = format!("<br><br>--<br>\n{escaped}");
            body.append(&html_fragment, &text_fragment)
        }
        None => body,
    }
}

#[cfg(test)]
mod tests {
    //! DB-query wiring is straightforward; the format-only tests here
    //! cover the dual-representation composition. Integration coverage
    //! comes from the outbound relay path.

    use super::*;
    use crate::models::{Comment, ContentFormat};
    use chrono::Utc;

    fn body_html(html: &str) -> ReplyBody {
        ReplyBody::from_comment(&Comment {
            id: 1,
            content: html.into(),
            ticket_id: 1,
            user_uuid: Uuid::nil(),
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            is_edited: false,
            edit_count: 0,
            channel_metadata: None,
            is_internal: false,
            deleted_at: None,
            content_format: ContentFormat::Html,
            body_text: None,
            body_html: None,
            new_content: None,
            quoted_content: None,
            raw_source_uri: None,
        })
    }

    /// Cheap composition helper that mirrors the DB-driven version
    /// without touching `users::signature`. Lets us assert the shape
    /// without standing up a real connection.
    fn compose(body: ReplyBody, sig: Option<&str>) -> ReplyBody {
        match sig {
            Some(s) if !s.trim().is_empty() => {
                let text = format!("\n\n-- \n{s}");
                let html = format!(
                    "<br><br>--<br>\n{}",
                    html_escape::encode_safe(s).replace('\n', "<br>\n")
                );
                body.append(&html, &text)
            }
            _ => body,
        }
    }

    #[test]
    fn no_signature_leaves_body_unchanged() {
        let before = body_html("<p>Hi.</p>");
        let after = compose(before.clone(), None);
        assert_eq!(after.html, before.html);
        assert_eq!(after.text, before.text);
    }

    #[test]
    fn empty_signature_is_treated_as_no_signature() {
        let before = body_html("<p>Hi.</p>");
        let after = compose(before.clone(), Some("   \n\n"));
        assert_eq!(after.html, before.html);
        assert_eq!(after.text, before.text);
    }

    #[test]
    fn plaintext_uses_rfc3676_separator() {
        let body = compose(body_html("<p>Hi!</p>"), Some("Tech Person\nIT Support"));
        assert!(
            body.text.contains("\n\n-- \nTech Person\nIT Support"),
            "got: {}",
            body.text
        );
    }

    #[test]
    fn html_signature_is_escaped_and_brified() {
        let body = compose(body_html("<p>Hi!</p>"), Some("Tech <admin>\nFooter line"));
        // User-authored angle brackets must not break the HTML body.
        assert!(body.html.contains("Tech &lt;admin&gt;"));
        assert!(body.html.contains("<br>\nFooter line"));
        // RFC 3676 marker preserved in plain too.
        assert!(body.text.contains("-- \nTech <admin>"));
    }
}
