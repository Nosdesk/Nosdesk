//! What each viewer sees of a person's row.
//!
//! Every member of a workspace receives its people, but not every field of
//! them. Of someone else, everyone gets the name and avatar, staff also the
//! email, the workspace role and their working details, and only the person
//! themselves their platform role and preferences. [`for_viewer`] is the one
//! place that says so: the sync read paths (bootstrap, delta, the live
//! stream) apply it to `user` records, and the user routes to the rows they
//! return, so the two can't drift apart.
//!
//! It works on the row's JSON object, as sync records it and as the routes
//! serialize `UserResponse`, because those carry different fields. A field
//! the viewer may not see is left out, not nulled: null would read as "this
//! person has none". A field not named in [`reach`] is the person's own
//! until someone says otherwise.

use serde_json::{Map, Value};
use uuid::Uuid;

/// The viewer a row is projected for.
#[derive(Debug, Clone, Copy)]
pub struct Viewer {
    pub uuid: Uuid,
    /// Workspace agent and up, or platform admin.
    pub is_staff: bool,
}

/// Who may see one field of a person's row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Everyone in the workspace.
    Everyone,
    /// Staff, and the person themselves.
    Staff,
    /// Only the person themselves.
    Owner,
}

/// Who may see `field` of a person's row.
pub fn reach(field: &str) -> Reach {
    match field {
        // Who someone is, as every ticket and comment shows them.
        "uuid" | "name" | "pronouns" | "avatar_url" | "avatar_thumb" | "banner_url"
        | "deleted_at" => Reach::Everyone,
        // The caller's own rights over the row (`GET /users/{uuid}`), not
        // the person's data.
        "editable" => Reach::Everyone,
        // What staff work with: how to reach someone, their role here, their
        // load, and who manages their account.
        "email" | "workspace_role" | "created_at" | "updated_at" | "open_ticket_count"
        | "device_count" | "managed_by" | "microsoft_uuid" => Reach::Staff,
        // Platform role, preferences and anything not named above.
        _ => Reach::Owner,
    }
}

/// `row`, a person's row, as `viewer` may see it: the fields they may not
/// see are removed. The person is `row["uuid"]`; a row that doesn't name one
/// is treated as someone else's.
pub fn for_viewer(row: &mut Map<String, Value>, viewer: Viewer) {
    let own = row
        .get("uuid")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .is_some_and(|subject| subject == viewer.uuid);
    if own {
        return;
    }
    row.retain(|field, _| match reach(field) {
        Reach::Everyone => true,
        Reach::Staff => viewer.is_staff,
        Reach::Owner => false,
    });
}

/// [`for_viewer`] on a JSON value; anything but an object is left alone.
pub fn value_for_viewer(row: &mut Value, viewer: Viewer) {
    if let Value::Object(map) = row {
        for_viewer(map, viewer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(uuid: Uuid) -> Map<String, Value> {
        json!({
            "uuid": uuid,
            "name": "Sam Rivera",
            "pronouns": "they/them",
            "avatar_url": "/a.png",
            "avatar_thumb": "/a-thumb.png",
            "deleted_at": null,
            "email": "sam@example.test",
            "workspace_role": "agent",
            "platform_role": "audit_reviewer",
            "dashboard_layout": { "widgets": [] },
            "signature": "Sam",
            "theme": "dark",
            "a_field_from_a_later_version": 1,
        })
        .as_object()
        .cloned()
        .unwrap_or_default()
    }

    fn keys(map: &Map<String, Value>) -> Vec<&str> {
        let mut k: Vec<&str> = map.keys().map(String::as_str).collect();
        k.sort_unstable();
        k
    }

    #[test]
    fn a_person_sees_their_whole_row() {
        let me = Uuid::new_v4();
        let mut mine = row(me);
        for_viewer(
            &mut mine,
            Viewer {
                uuid: me,
                is_staff: false,
            },
        );
        assert_eq!(mine, row(me));
    }

    #[test]
    fn staff_see_how_to_reach_someone_but_not_their_own_settings() {
        let mut theirs = row(Uuid::new_v4());
        for_viewer(
            &mut theirs,
            Viewer {
                uuid: Uuid::new_v4(),
                is_staff: true,
            },
        );
        assert_eq!(
            keys(&theirs),
            vec![
                "avatar_thumb",
                "avatar_url",
                "deleted_at",
                "email",
                "name",
                "pronouns",
                "uuid",
                "workspace_role"
            ]
        );
    }

    #[test]
    fn a_member_sees_others_name_and_avatar() {
        let mut theirs = row(Uuid::new_v4());
        for_viewer(
            &mut theirs,
            Viewer {
                uuid: Uuid::new_v4(),
                is_staff: false,
            },
        );
        assert_eq!(
            keys(&theirs),
            vec![
                "avatar_thumb",
                "avatar_url",
                "deleted_at",
                "name",
                "pronouns",
                "uuid"
            ]
        );
    }
}
