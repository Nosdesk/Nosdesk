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
//! Resolution chain mirrors locale / timezone (see
//! `user_preferences.rs`):
//!   `user_preferences.signature` (trimmed, non-empty)
//!     → `site_settings.signature_default` (trimmed, non-empty)
//!     → no signature (reply goes out unsigned)
//!
//! The org default lets a workspace set one team-wide template and
//! have every agent who hasn't customised their own fall through to
//! it, matching Zendesk / Help Scout / Freshdesk semantics.

use uuid::Uuid;

use super::reply_body::ReplyBody;
use crate::db::DbConnection;

/// Trim a stored signature, returning `None` for empty / whitespace.
/// The trim only gates the empty-check; the returned string keeps
/// the original leading / trailing whitespace so admin-authored
/// indentation isn't silently stripped.
fn non_empty(raw: Option<String>) -> Option<String> {
    raw.and_then(|s| if s.trim().is_empty() { None } else { Some(s) })
}

/// Fetch the resolved signature for an agent, walking
/// per-user → org default → none. Per-source DB read failures are
/// silently treated as "no value at this level" — better to send
/// the reply unsigned than to fail the whole outbound dispatch
/// over a transient read hiccup.
fn signature_for_user(conn: &mut DbConnection, user_uuid: Uuid) -> Option<String> {
    if let Some(s) = non_empty(
        crate::repository::user_preferences::get_signature(conn, user_uuid)
            .ok()
            .flatten(),
    ) {
        return Some(s);
    }
    non_empty(
        crate::repository::site_settings::get_site_settings(conn)
            .ok()
            .and_then(|s| s.signature_default),
    )
}

/// Render a signature template against the agent context. Templates
/// without `{{...}}` tokens short-circuit (zero extra queries on the
/// common "fixed text" case). When at least one token is present we
/// load the agent name + primary email + workspace `app_name` for
/// substitution; if the agent row can't be loaded we return `None`
/// so the caller skips appending altogether (a literal `{{tech_name}}`
/// in a customer reply is worse than no signature at all).
fn render_signature(conn: &mut DbConnection, user_uuid: Uuid, template: String) -> Option<String> {
    if !template.contains("{{") {
        return Some(template);
    }
    let user = crate::repository::users::get_user_by_uuid(&user_uuid, conn).ok()?;
    let tech_email =
        crate::repository::user_helpers::get_primary_email(&user_uuid, conn).unwrap_or_default();
    let app_name = crate::repository::site_settings::get_site_settings(conn)
        .map(|s| s.app_name)
        .unwrap_or_else(|_| "Nosdesk".to_string());
    let tech_first_name = crate::utils::template_variables::first_name(&user.name);
    Some(crate::utils::template_variables::substitute(
        &template,
        &[
            ("tech_name", &user.name),
            ("tech_first_name", &tech_first_name),
            ("tech_email", &tech_email),
            ("app_name", &app_name),
        ],
    ))
}

/// Append the user's signature to both representations of `body`.
///
/// DB read failures on the per-user / org-default lookups are
/// silently treated as "no signature" so the reply still ships; a
/// failed render against an existing template (e.g. transient user
/// read failure) drops the signature for the same reason.
pub fn append_signature_for_user(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    body: ReplyBody,
) -> ReplyBody {
    let template = match signature_for_user(conn, user_uuid) {
        Some(s) => s,
        None => return body,
    };
    let sig = match render_signature(conn, user_uuid, template) {
        Some(s) => s,
        None => return body,
    };
    let text_fragment = format!("\n\n-- \n{sig}");
    // Escape the user-authored signature before embedding into
    // HTML; their newlines become `<br>` so the visual layout
    // matches the plaintext.
    let escaped = html_escape::encode_safe(&sig).replace('\n', "<br>\n");
    let html_fragment = format!("<br><br>--<br>\n{escaped}");
    body.append(&html_fragment, &text_fragment)
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
            workspace_id: 1,
            render_kind: None,
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

    /// Pure-function mirror of the per-user → org-default → none
    /// resolution chain. Same precedence rule the live function uses
    /// against the DB; lets us cover the fallback without standing
    /// up a real connection.
    fn resolve(user_sig: Option<&str>, org_default: Option<&str>) -> Option<String> {
        if let Some(s) = user_sig {
            if !s.trim().is_empty() {
                return Some(s.to_string());
            }
        }
        org_default.and_then(|s| {
            if s.trim().is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
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

    #[test]
    fn user_signature_wins_over_org_default() {
        let sig = resolve(Some("Personal sig"), Some("Org default"));
        assert_eq!(sig.as_deref(), Some("Personal sig"));
    }

    #[test]
    fn org_default_used_when_user_unset() {
        let sig = resolve(None, Some("Org default"));
        assert_eq!(sig.as_deref(), Some("Org default"));
    }

    #[test]
    fn org_default_used_when_user_blank() {
        let sig = resolve(Some("   \n  "), Some("Org default"));
        assert_eq!(sig.as_deref(), Some("Org default"));
    }

    #[test]
    fn no_signature_when_both_unset() {
        assert!(resolve(None, None).is_none());
    }

    #[test]
    fn no_signature_when_both_blank() {
        assert!(resolve(Some("  "), Some("\n\n")).is_none());
    }
}
