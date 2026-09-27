//! `GET /api/portal/events`: live hints for the requester portal.
//!
//! The agent event feed carries agent-shaped rows; the portal never sees them.
//! This stream authenticates with the portal session cookie (no token in the
//! URL), listens to the workspace feed server-side, and sends only "request N
//! changed" hints for requests the requester can see, checked per event with
//! the same visibility the portal's reads use. The page then refetches through
//! the portal API, so a hint alone reveals nothing but an id they can already
//! open.

use std::collections::BTreeSet;
use std::time::Duration;

use actix_web::{web, HttpResponse};
use futures::stream::{self, StreamExt};
use serde_json::Value;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::db::Pool;
use crate::errors::{self, ApiError};
use crate::extractors::WorkspaceContext;
use crate::handlers::portal::PortalContext;
use crate::handlers::sse::{Envelope, SseEvent, SseState};

/// Keeps idle proxies from closing the stream.
const HEARTBEAT: Duration = Duration::from_secs(25);

/// Tickets a batch of sync rows touched that a requester could care about:
/// ticket changes, and comments that aren't internal notes. Everything else
/// (projects, assets, internal notes, staff-only rows) is ignored.
pub(crate) fn touched_tickets(actions: &Value) -> BTreeSet<i32> {
    let mut ids = BTreeSet::new();
    for row in actions.as_array().into_iter().flatten() {
        let aggregate = row.get("aggregate").and_then(Value::as_str).unwrap_or("");
        let data = row.get("data").unwrap_or(&Value::Null);
        let id = match aggregate {
            "ticket" => row
                .get("aggregate_id")
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok()),
            "comment" if data.get("is_internal").and_then(Value::as_bool) != Some(true) => data
                .get("ticket_id")
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok()),
            _ => None,
        };
        ids.extend(id);
    }
    ids
}

/// The subset of `ids` `viewer` can see in the portal now.
fn visible(pool: &Pool, workspace_id: i32, viewer: Uuid, ids: BTreeSet<i32>) -> Vec<i32> {
    crate::sync::session::run_in_workspace(
        pool,
        "background:portal_events_visibility",
        workspace_id,
        move |conn| {
            let vis = crate::handlers::portal::portal_visibility(conn, viewer)?;
            let mut out = Vec::new();
            for id in ids {
                if crate::repository::ticket_visibility::can_view_ticket(conn, &vis, id)? {
                    out.push(id);
                }
            }
            Ok::<_, diesel::result::Error>(out)
        },
    )
    .unwrap_or_default()
}

enum Next {
    Event(Result<Envelope, broadcast::error::RecvError>),
    Tick,
}

pub async fn portal_events(
    portal: PortalContext,
    ws: WorkspaceContext,
    state: web::Data<SseState>,
    pool: web::Data<Pool>,
) -> Result<HttpResponse, ApiError> {
    let viewer = portal.user_uuid;
    let workspace_id = ws.workspace_id;
    let Some(guard) =
        crate::services::connection_registry::global().try_acquire((viewer, workspace_id))
    else {
        return Ok(errors::too_many_requests("Too many requests", 5));
    };
    let rx = state.subscribe_workspace(workspace_id);
    let pool = pool.get_ref().clone();

    let open =
        stream::once(async { Ok::<_, actix_web::Error>(web::Bytes::from("retry: 5000\n\n")) });
    let hints = stream::unfold(
        (rx, pool, tokio::time::interval(HEARTBEAT), guard),
        move |(mut rx, pool, mut tick, guard)| async move {
            loop {
                let next = tokio::select! {
                    ev = rx.recv() => Next::Event(ev),
                    _ = tick.tick() => Next::Tick,
                };
                let chunk = match next {
                    Next::Tick => Some(": ping\n\n".to_string()),
                    Next::Event(Ok(Envelope {
                        event: SseEvent::SyncActions { actions, .. },
                        ..
                    })) => {
                        let ids = touched_tickets(&actions);
                        if ids.is_empty() {
                            None
                        } else {
                            let p = pool.clone();
                            let shown = web::block(move || visible(&p, workspace_id, viewer, ids))
                                .await
                                .unwrap_or_default();
                            (!shown.is_empty()).then(|| {
                                shown
                                    .into_iter()
                                    .map(|id| format!("event: ticket\ndata: {{\"id\":{id}}}\n\n"))
                                    .collect()
                            })
                        }
                    }
                    // Missed events: tell the page to refetch everything.
                    Next::Event(Err(broadcast::error::RecvError::Lagged(_))) => {
                        Some("event: resync\ndata: {}\n\n".to_string())
                    }
                    Next::Event(Err(broadcast::error::RecvError::Closed)) => return None,
                    Next::Event(Ok(_)) => None,
                };
                if let Some(chunk) = chunk {
                    return Some((
                        Ok::<_, actix_web::Error>(web::Bytes::from(chunk)),
                        (rx, pool, tick, guard),
                    ));
                }
            }
        },
    );

    Ok(HttpResponse::Ok()
        .append_header(("Content-Type", "text/event-stream"))
        .append_header(("Cache-Control", "no-cache"))
        .append_header(("X-Accel-Buffering", "no"))
        .streaming(open.chain(hints)))
}

#[cfg(test)]
mod tests {
    use super::touched_tickets;
    use serde_json::json;

    #[test]
    fn hints_name_tickets_and_public_comments_only() {
        let actions = json!([
            { "aggregate": "ticket", "aggregate_id": "12", "data": {} },
            { "aggregate": "comment", "aggregate_id": "5", "data": { "ticket_id": 13, "is_internal": false } },
            { "aggregate": "comment", "aggregate_id": "6", "data": { "ticket_id": 14, "is_internal": true } },
            { "aggregate": "project", "aggregate_id": "3", "data": {} },
            { "aggregate": "ticket", "aggregate_id": "12", "data": {} }
        ]);
        assert_eq!(
            touched_tickets(&actions).into_iter().collect::<Vec<_>>(),
            vec![12, 13]
        );
    }
}
