//! Notifications derived from `sync_actions`.
//!
//! A ticket assignment, a status change and a comment are facts recorded in
//! the event log by every write path (REST, sync push, bulk, automations).
//! Deriving the notification from the row, rather than from whichever handler
//! happened to perform the write, is what keeps a new write path from silently
//! losing it. `docs/plans/notifications-from-sync-actions.md` has the design.
//!
//! Two stages. [`derive`] is pure and reads only the row: it produces
//! [`Intent`]s and is unit tested on literal rows. [`resolve`] turns an intent
//! into payloads and is the only part that queries (actor display, watchers,
//! the internal-note staff gate, status categories).

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde_json::Value;
use uuid::Uuid;

use super::mentions::{parse_mentions, strip_html_for_preview, truncate_preview};
use super::types::{
    NotificationActor, NotificationEntity, NotificationPayload, NotificationTypeCode,
};
use crate::db::DbConnection;
use crate::sync::ActorKind;

/// The columns of a `sync_actions` row the deriver reads.
#[derive(Debug, Clone)]
pub struct SyncActionRow {
    pub sync_id: i64,
    pub workspace_id: i32,
    pub event_type: String,
    pub data: Value,
    pub actor_uuid: Option<Uuid>,
    pub actor_kind: String,
    pub occurred_at: DateTime<Utc>,
}

/// What a row means for notifications, before anyone is looked up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// `assignee` now owns the ticket and did not do it themselves.
    Assigned {
        ticket_id: i32,
        ticket_title: String,
        assignee: Uuid,
    },
    /// The workflow state moved; whether the requester hears about it depends
    /// on the state *category* changing, which needs a lookup.
    StatusChanged {
        ticket_id: i32,
        ticket_title: String,
        requester: Uuid,
        previous_state_id: i32,
        state_id: i32,
    },
    /// A comment landed. Recipients and mentions are resolved against the
    /// ticket's watchers and, for an internal note, its staff.
    Commented {
        ticket_id: i32,
        ticket_title: String,
        comment_id: i32,
        commenter: Uuid,
        is_internal: bool,
        requester: Option<Uuid>,
        assignee: Option<Uuid>,
        mentions: Vec<Uuid>,
        preview: String,
    },
}

fn uuid_at(data: &Value, key: &str) -> Option<Uuid> {
    data.get(key)
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
}

fn i32_at(data: &Value, key: &str) -> Option<i32> {
    data.get(key)
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
}

fn str_at<'a>(data: &'a Value, key: &str) -> Option<&'a str> {
    data.get(key).and_then(Value::as_str)
}

/// Pure: the intents a row carries. Keyed on the before/after pairs in
/// `data`, not on `event_type`, because a combined update is labelled by its
/// most specific field and would otherwise mask the assignment underneath.
pub fn derive(row: &SyncActionRow) -> Vec<Intent> {
    let data = &row.data;
    let mut intents = Vec::new();

    if row.event_type == "comment.created" {
        let commenter = uuid_at(data, "user_uuid");
        let (Some(comment_id), Some(ticket_id), Some(commenter)) =
            (i32_at(data, "id"), i32_at(data, "ticket_id"), commenter)
        else {
            return intents;
        };
        let content = str_at(data, "content").unwrap_or_default();
        let mentions = parse_mentions(content)
            .into_iter()
            .filter(|u| *u != commenter)
            .collect();
        intents.push(Intent::Commented {
            ticket_id,
            ticket_title: str_at(data, "ticket_title").unwrap_or_default().to_string(),
            comment_id,
            commenter,
            is_internal: data
                .get("is_internal")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            requester: uuid_at(data, "ticket_requester_uuid"),
            assignee: uuid_at(data, "ticket_assignee_uuid"),
            mentions,
            preview: truncate_preview(&strip_html_for_preview(content), 100),
        });
        return intents;
    }

    // Ticket rows. Both keys are only ever written by the writes that can
    // change the field; their presence is the signal, their value the
    // comparison.
    let Some(ticket_id) = i32_at(data, "id") else {
        return intents;
    };
    let ticket_title = str_at(data, "title").unwrap_or_default().to_string();

    if data.get("previous_assignee_uuid").is_some() {
        let previous = uuid_at(data, "previous_assignee_uuid");
        let current = uuid_at(data, "assignee_uuid");
        if let Some(assignee) = current {
            if Some(assignee) != previous && Some(assignee) != row.actor_uuid {
                intents.push(Intent::Assigned {
                    ticket_id,
                    ticket_title: ticket_title.clone(),
                    assignee,
                });
            }
        }
    }

    if data.get("previous_workflow_state_id").is_some() {
        let previous = i32_at(data, "previous_workflow_state_id");
        let current = i32_at(data, "workflow_state_id");
        if let (Some(previous_state_id), Some(state_id)) = (previous, current) {
            if previous_state_id != state_id {
                if let Some(requester) = uuid_at(data, "requester_uuid") {
                    if Some(requester) != row.actor_uuid {
                        intents.push(Intent::StatusChanged {
                            ticket_id,
                            ticket_title,
                            requester,
                            previous_state_id,
                            state_id,
                        });
                    }
                }
            }
        }
    }

    intents
}

/// The notification actor for a row. A human actor is looked up for their
/// display name; system and plugin actors carry none.
fn actor_for(conn: &mut DbConnection, row: &SyncActionRow) -> NotificationActor {
    let kind = match row.actor_kind.as_str() {
        "system" => ActorKind::System,
        "plugin" => ActorKind::Plugin,
        _ => ActorKind::User,
    };
    if kind == ActorKind::User {
        if let Some(uuid) = row.actor_uuid {
            if let Ok(user) = crate::repository::get_user_by_uuid(&uuid, conn) {
                return NotificationActor {
                    uuid,
                    name: user.name,
                    avatar_thumb: user.avatar_thumb,
                    kind,
                };
            }
        }
    }
    NotificationActor {
        uuid: row.actor_uuid.unwrap_or_else(Uuid::nil),
        name: match kind {
            ActorKind::Plugin => "Plugin",
            _ => "System",
        }
        .to_string(),
        avatar_thumb: None,
        kind,
    }
}

/// Staff of `workspace_id` among `candidates`: platform admins, or members
/// with an owner/admin/agent seat. Scoped to *this* workspace; the earlier
/// hard-coded workspace 1 stripped every internal-note recipient elsewhere.
fn staff_among(
    conn: &mut DbConnection,
    workspace_id: i32,
    candidates: &[Uuid],
) -> QueryResult<HashSet<Uuid>> {
    use crate::schema::{users, workspace_members};
    if candidates.is_empty() {
        return Ok(HashSet::new());
    }
    users::table
        .filter(users::uuid.eq_any(candidates))
        .filter(
            users::platform_role
                .eq("platform_admin")
                .or(diesel::dsl::exists(
                    workspace_members::table
                        .filter(workspace_members::user_uuid.eq(users::uuid))
                        .filter(workspace_members::workspace_id.eq(workspace_id))
                        .filter(workspace_members::role.eq_any(vec!["owner", "admin", "agent"]))
                        .filter(workspace_members::removed_at.is_null()),
                )),
        )
        .select(users::uuid)
        .load::<Uuid>(conn)
        .map(|v| v.into_iter().collect())
}

/// Payloads for one intent. Runs on a connection pinned to the row's
/// workspace, so watcher and membership reads are RLS-scoped.
pub fn resolve(
    conn: &mut DbConnection,
    row: &SyncActionRow,
    intent: Intent,
) -> QueryResult<Vec<NotificationPayload>> {
    let workspace_id = row.workspace_id;
    let mut out = Vec::new();
    match intent {
        Intent::Assigned {
            ticket_id,
            ticket_title,
            assignee,
        } => {
            let actor = actor_for(conn, row);
            let body = if actor.kind == ActorKind::User {
                format!("You have been assigned to ticket #{ticket_id}")
            } else {
                format!("You have been auto-assigned to ticket #{ticket_id}")
            };
            out.push(
                NotificationPayload::new(
                    NotificationTypeCode::TicketAssigned,
                    assignee,
                    actor,
                    NotificationEntity::Ticket {
                        id: ticket_id,
                        title: ticket_title,
                    },
                    workspace_id,
                )
                .with_body(body)
                .from_sync_action(row.sync_id),
            );
        }
        Intent::StatusChanged {
            ticket_id,
            ticket_title,
            requester,
            previous_state_id,
            state_id,
        } => {
            // The requester hears about a change of *category* (open to
            // resolved), not a move between two states of the same kind.
            let category_of = |conn: &mut DbConnection, id: i32| {
                crate::repository::workflow_states::category_of(conn, id)
                    .ok()
                    .flatten()
                    .map(|c| c.as_str())
                    .unwrap_or("backlog")
            };
            let before = category_of(conn, previous_state_id);
            let after = category_of(conn, state_id);
            if before != after {
                let actor = actor_for(conn, row);
                out.push(
                    NotificationPayload::new(
                        NotificationTypeCode::TicketStatusChanged,
                        requester,
                        actor,
                        NotificationEntity::Ticket {
                            id: ticket_id,
                            title: ticket_title,
                        },
                        workspace_id,
                    )
                    .with_body(format!("Ticket #{ticket_id} status changed to {after}"))
                    .from_sync_action(row.sync_id),
                );
            }
        }
        Intent::Commented {
            ticket_id,
            ticket_title,
            comment_id,
            commenter,
            is_internal,
            requester,
            assignee,
            mut mentions,
            preview,
        } => {
            // Recipients, in the order the handler built them: requester,
            // assignee, then watchers; never the commenter, never someone
            // already being mentioned.
            let mut recipients: Vec<Uuid> = Vec::new();
            for candidate in [requester, assignee].into_iter().flatten() {
                if candidate != commenter
                    && !recipients.contains(&candidate)
                    && !mentions.contains(&candidate)
                {
                    recipients.push(candidate);
                }
            }
            let watchers = if is_internal {
                crate::repository::ticket_watchers::watcher_uuids_for_internal_notify(
                    conn, ticket_id,
                )
            } else {
                crate::repository::ticket_watchers::watcher_uuids(conn, ticket_id)
            }
            .unwrap_or_default();
            for watcher in watchers {
                if watcher != commenter
                    && !recipients.contains(&watcher)
                    && !mentions.contains(&watcher)
                {
                    recipients.push(watcher);
                }
            }

            // An internal note is staff-only. Without this gate a requester
            // mentioned in one would be told about a comment they cannot see.
            if is_internal {
                let candidates: Vec<Uuid> =
                    recipients.iter().chain(mentions.iter()).copied().collect();
                let staff = staff_among(conn, workspace_id, &candidates)?;
                recipients.retain(|u| staff.contains(u));
                mentions.retain(|u| staff.contains(u));
            }

            let actor = actor_for(conn, row);
            let entity = || NotificationEntity::Comment {
                id: comment_id,
                ticket_id,
                ticket_title: ticket_title.clone(),
            };
            for recipient in recipients {
                out.push(
                    NotificationPayload::new(
                        NotificationTypeCode::CommentAdded,
                        recipient,
                        actor.clone(),
                        entity(),
                        workspace_id,
                    )
                    .with_body(&preview)
                    .from_sync_action(row.sync_id),
                );
            }
            for mentioned in mentions {
                out.push(
                    NotificationPayload::new(
                        NotificationTypeCode::Mentioned,
                        mentioned,
                        actor.clone(),
                        entity(),
                        workspace_id,
                    )
                    .with_body(&preview)
                    .from_sync_action(row.sync_id),
                );
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const A: &str = "0192aaaa-0000-7000-8000-00000000000a";
    const B: &str = "0192aaaa-0000-7000-8000-00000000000b";
    const C: &str = "0192aaaa-0000-7000-8000-00000000000c";

    fn u(s: &str) -> Uuid {
        Uuid::parse_str(s).unwrap()
    }

    fn row(event_type: &str, data: Value, actor: Option<&str>) -> SyncActionRow {
        SyncActionRow {
            sync_id: 42,
            workspace_id: 1,
            event_type: event_type.to_string(),
            data,
            actor_uuid: actor.map(u),
            actor_kind: "user".into(),
            occurred_at: Utc::now(),
        }
    }

    #[test]
    fn assignment_derives_when_the_pair_differs() {
        let r = row(
            "ticket.assignee_changed",
            json!({"id": 7, "title": "T", "assignee_uuid": A, "previous_assignee_uuid": null}),
            Some(B),
        );
        assert_eq!(
            derive(&r),
            vec![Intent::Assigned {
                ticket_id: 7,
                ticket_title: "T".into(),
                assignee: u(A)
            }]
        );
    }

    #[test]
    fn a_full_row_without_the_previous_key_is_not_an_assignment() {
        // Every ticket emitter carries assignee_uuid; only the writes that can
        // change it carry previous_assignee_uuid.
        let r = row(
            "ticket.tag_ids",
            json!({"id": 7, "title": "T", "assignee_uuid": A}),
            Some(B),
        );
        assert!(derive(&r).is_empty());
    }

    #[test]
    fn unchanged_assignee_is_not_an_assignment() {
        let r = row(
            "ticket.updated",
            json!({"id": 7, "title": "T", "assignee_uuid": A, "previous_assignee_uuid": A}),
            Some(B),
        );
        assert!(derive(&r).is_empty());
    }

    #[test]
    fn self_assignment_is_silent() {
        let r = row(
            "ticket.assignee_changed",
            json!({"id": 7, "title": "T", "assignee_uuid": A, "previous_assignee_uuid": null}),
            Some(A),
        );
        assert!(derive(&r).is_empty());
    }

    #[test]
    fn unassignment_is_silent() {
        let r = row(
            "ticket.assignee_changed",
            json!({"id": 7, "title": "T", "assignee_uuid": null, "previous_assignee_uuid": A}),
            Some(B),
        );
        assert!(derive(&r).is_empty());
    }

    #[test]
    fn combined_update_labelled_by_status_still_derives_the_assignment() {
        let r = row(
            "ticket.workflow_state_changed",
            json!({
                "id": 7, "title": "T", "requester_uuid": C,
                "assignee_uuid": A, "previous_assignee_uuid": null,
                "workflow_state_id": 3, "previous_workflow_state_id": 2
            }),
            Some(B),
        );
        let intents = derive(&r);
        assert_eq!(intents.len(), 2);
        assert!(matches!(intents[0], Intent::Assigned { assignee, .. } if assignee == u(A)));
        assert!(matches!(
            intents[1],
            Intent::StatusChanged { requester, previous_state_id: 2, state_id: 3, .. } if requester == u(C)
        ));
    }

    #[test]
    fn status_change_by_the_requester_is_silent() {
        let r = row(
            "ticket.workflow_state_changed",
            json!({
                "id": 7, "title": "T", "requester_uuid": C,
                "workflow_state_id": 3, "previous_workflow_state_id": 2
            }),
            Some(C),
        );
        assert!(derive(&r).is_empty());
    }

    #[test]
    fn created_with_assignee_is_an_assignment_and_not_a_status_change() {
        let r = row(
            "ticket.created",
            json!({"id": 7, "title": "T", "assignee_uuid": A, "previous_assignee_uuid": null,
                   "workflow_state_id": 1, "requester_uuid": C}),
            Some(B),
        );
        let intents = derive(&r);
        assert_eq!(intents.len(), 1);
        assert!(matches!(intents[0], Intent::Assigned { .. }));
    }

    #[test]
    fn comment_carries_mentions_in_both_syntaxes_minus_the_commenter() {
        let content = format!(
            r#"<p><span data-mention="true" data-uuid="{A}" data-name="Alex">@Alex</span> and @[Bo]({B}) and @[Me]({C})</p>"#
        );
        let r = row(
            "comment.created",
            json!({
                "id": 9, "ticket_id": 7, "user_uuid": C, "is_internal": true,
                "content": content, "ticket_title": "T",
                "ticket_requester_uuid": B, "ticket_assignee_uuid": A
            }),
            Some(C),
        );
        let intents = derive(&r);
        assert_eq!(intents.len(), 1);
        let Intent::Commented {
            ref mentions,
            is_internal,
            requester,
            assignee,
            ref preview,
            ..
        } = intents[0]
        else {
            panic!("expected Commented");
        };
        assert_eq!(mentions, &vec![u(A), u(B)]);
        assert!(is_internal);
        assert_eq!(requester, Some(u(B)));
        assert_eq!(assignee, Some(u(A)));
        assert_eq!(preview, "@Alex and @Bo and @Me");
    }

    #[test]
    fn comment_without_identifiers_derives_nothing() {
        let r = row("comment.created", json!({"content": "orphan"}), Some(C));
        assert!(derive(&r).is_empty());
    }
}
