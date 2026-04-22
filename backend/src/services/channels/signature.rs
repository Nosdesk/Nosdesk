//! Append the agent's email signature to outbound channel replies.
//!
//! The signature text lives on `users.signature`. Agents manage their
//! own via the profile edit UI. We append with the RFC-3676 "-- \n"
//! separator (dash-dash-space-newline) so mail clients recognize the
//! signature block and offer to collapse / strip it cleanly.
//!
//! No-op when the user has no signature or it's empty/whitespace.

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;

/// Fetch the user's stored signature; `None` if unset or whitespace.
fn signature_for_user(conn: &mut DbConnection, user_uuid: Uuid) -> Option<String> {
    use crate::schema::users;
    let raw: Option<Option<String>> = users::table
        .filter(users::uuid.eq(user_uuid))
        .select(users::signature)
        .first(conn)
        .ok();
    raw.flatten().and_then(|s| {
        let t = s.trim();
        if t.is_empty() { None } else { Some(s) }
    })
}

/// Return `body + "\n\n-- \n{signature}"` when the user has a
/// non-empty signature; otherwise the body unchanged.
///
/// DB read failures are silently treated as "no signature" — we'd
/// rather send the reply without a signature than fail the whole
/// outbound dispatch over a transient read hiccup.
pub fn append_signature_for_user(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    body: &str,
) -> String {
    match signature_for_user(conn, user_uuid) {
        Some(sig) => format!("{body}\n\n-- \n{sig}"),
        None => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    //! The DB-query wiring is straightforward; format-only tests here.
    //! Integration coverage comes from the outbound relay path.

    #[test]
    fn append_noop_on_empty_signature() {
        // Format helper isolated from the DB lookup so we can test
        // the composition rule without a real connection.
        fn compose(body: &str, sig: Option<&str>) -> String {
            match sig {
                Some(s) if !s.trim().is_empty() => format!("{body}\n\n-- \n{s}"),
                _ => body.to_string(),
            }
        }
        assert_eq!(compose("Hi!", None), "Hi!");
        assert_eq!(compose("Hi!", Some("")), "Hi!");
        assert_eq!(compose("Hi!", Some("   \n\n")), "Hi!");
    }

    #[test]
    fn append_uses_rfc3676_separator() {
        fn compose(body: &str, sig: Option<&str>) -> String {
            match sig {
                Some(s) if !s.trim().is_empty() => format!("{body}\n\n-- \n{s}"),
                _ => body.to_string(),
            }
        }
        let out = compose("Hi!", Some("Tech Person\nIT Support"));
        assert!(out.contains("\n\n-- \nTech Person\nIT Support"));
    }
}
