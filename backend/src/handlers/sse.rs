use actix_web::{web, HttpRequest, HttpResponse, Result as ActixResult};

use crate::handlers::errors;
use dashmap::DashMap;
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::time::interval;
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

// Event types for SSE
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SseEvent {
    /// Per-user presence on a ticket. Replaces the v0
    /// `ViewerCountChanged` event: instead of a bare count, ships
    /// the deduplicated viewer set so the frontend can render
    /// avatars and filter out the current user. Routed to
    /// `TopicKey::Ticket(ticket_id)` so only subscribers
    /// authorised on that ticket receive it; the per-user
    /// identities here are visibility-sensitive.
    ViewersChanged {
        ticket_id: i32,
        viewers: Vec<crate::services::presence::ViewerInfo>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Transient in-flight edit on a single ticket field. Broadcast
    /// during typing so other viewers' UI mirrors the keystrokes
    /// without the backend writing to the DB or emitting a
    /// `sync_actions` activity row. The PATCH commit path remains
    /// the only writer; this channel exists purely to decouple
    /// real-time mirroring from persistence. Routed to
    /// `TopicKey::Ticket(ticket_id)` like `ViewersChanged`.
    TicketFieldPreviewed {
        ticket_id: i32,
        field: String,
        value: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    Heartbeat {
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Sync engine outbox: a batch of `sync_actions` rows that
    /// committed since the last frame. Carried as a JSON value to
    /// avoid pulling the strongly-typed ActionRow into this enum
    /// (which would force every consumer of SseEvent to compile
    /// against the sync handler module). Frontend's sync runtime
    /// listens for this variant and feeds the payload into the
    /// object pool.
    SyncActions {
        actions: serde_json::Value,
        /// Commit-safe cursor for the batch: `(last_xid8, last_sync_id)`.
        /// Clients advance their feed position by both — see
        /// `crate::sync::feed`.
        last_xid8: i64,
        last_sync_id: i64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
}

fn event_type_str(event: &SseEvent) -> &'static str {
    match event {
        SseEvent::ViewersChanged { .. } => "viewers-changed",
        SseEvent::TicketFieldPreviewed { .. } => "ticket-field-previewed",
        SseEvent::Heartbeat { .. } => "heartbeat",
        SseEvent::SyncActions { .. } => "sync-actions",
    }
}

// Client connection info
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ClientInfo {
    pub user_id: String,
    pub connected_at: Instant,
    pub last_ping: Instant,
}

/// Routing key for SSE delivery. Each event lives on exactly one topic;
/// clients subscribe to the topics they care about and only ever see
/// events published there. The topology is small: `Global` carries
/// cross-resource events that aren't tenant data, `Workspace(id)`
/// carries the live `SyncActions` feed for one workspace, `User(uuid)`
/// carries targeted notifications, and `Ticket(id)` carries
/// presence-style events whose payload is sensitive enough that only
/// authorised subscribers may receive them.
///
/// Subscription to `Ticket(id)` is gated by
/// `ticket_visibility::can_view_ticket` at connect time (see
/// `parse_topics_authorized`), so the visibility filter is structural
/// — it doesn't run per event. Subscription to `Workspace(id)` is not
/// client-selectable at all: the connection is subscribed to exactly
/// its pinned, membership-gated workspace (see `sse_events_stream`),
/// which is what keeps one tenant's live sync rows off another
/// tenant's wire.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TopicKey {
    Global,
    Workspace(i32),
    User(String),
    Ticket(i32),
}

/// One delivery on the wire. `id` is a process-monotonic sequence so
/// reconnecting clients can ask for "anything after id N" via the
/// standard EventSource `Last-Event-ID` header. `source_client_id` is
/// optional echo-suppression metadata: the publishing connection
/// ignores envelopes it tagged itself.
#[derive(Clone, Debug)]
pub struct Envelope {
    pub id: u64,
    pub event: SseEvent,
    pub source_client_id: Option<String>,
}

/// Back-compat alias. `BroadcastMessage` was the prior name for the
/// wire envelope, before per-topic routing landed. Kept so existing
/// imports compile until callers move over.
pub type BroadcastMessage = Envelope;

/// Per-topic state: a broadcast sender for live tailing plus a small
/// ring of recent envelopes so a reconnecting client can replay events
/// it missed during the network gap. Ring is bounded — if a client is
/// gone for longer than the ring covers, the server drops the gap and
/// the frontend is expected to refetch.
struct TopicChannel {
    sender: broadcast::Sender<Envelope>,
    ring: Mutex<VecDeque<Envelope>>,
}

// Live broadcast tolerance per topic. Mirrors the prior 1000-event
// global buffer so a single slow client doesn't get disconnected
// during a normal burst. The replay ring is smaller and tracked
// separately — its only job is to cover short reconnect gaps.
const TOPIC_BUFFER: usize = 1024;
const TOPIC_RING: usize = 256;

impl TopicChannel {
    fn new() -> Self {
        let (sender, _) = broadcast::channel(TOPIC_BUFFER);
        Self {
            sender,
            ring: Mutex::new(VecDeque::with_capacity(TOPIC_RING)),
        }
    }

    fn record(&self, env: &Envelope) {
        let mut ring = self.ring.lock().unwrap();
        if ring.len() == TOPIC_RING {
            ring.pop_front();
        }
        ring.push_back(env.clone());
    }

    fn replay_after(&self, last_event_id: u64) -> Vec<Envelope> {
        let ring = self.ring.lock().unwrap();
        ring.iter()
            .filter(|e| e.id > last_event_id)
            .cloned()
            .collect()
    }
}

/// Topic-routed event bus. Replaces the prior single global broadcast
/// channel. Topics are created lazily on first publish or first
/// subscribe, so the map only holds keys we actually need.
pub struct SseState {
    topics: DashMap<TopicKey, Arc<TopicChannel>>,
    seq: AtomicU64,
    pub clients: Arc<Mutex<HashMap<String, ClientInfo>>>,
}

impl Default for SseState {
    fn default() -> Self {
        Self::new()
    }
}

impl SseState {
    pub fn new() -> Self {
        Self {
            topics: DashMap::new(),
            seq: AtomicU64::new(0),
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn topic(&self, key: TopicKey) -> Arc<TopicChannel> {
        self.topics
            .entry(key)
            .or_insert_with(|| Arc::new(TopicChannel::new()))
            .clone()
    }

    /// Pick the topic that owns this event. Notifications are
    /// per-recipient and must never appear on a wire other users read,
    /// so they live on a `User` topic. `SyncActions` carries tenant
    /// rows but no routing discriminant in the event itself, so it has
    /// no inferable topic — the outbox publishes each batch to its
    /// workspace's topic via [`broadcast_event_to`](Self::broadcast_event_to);
    /// returning `None` here fails closed if some future caller tries
    /// to publish one through the inferred-topic path (routing it to
    /// `Global` would put one tenant's rows on every tenant's wire).
    /// Everything else fans out on `Global`.
    fn topic_for(event: &SseEvent) -> Option<TopicKey> {
        match event {
            SseEvent::ViewersChanged { ticket_id, .. } => Some(TopicKey::Ticket(*ticket_id)),
            SseEvent::TicketFieldPreviewed { ticket_id, .. } => Some(TopicKey::Ticket(*ticket_id)),
            SseEvent::SyncActions { .. } => None,
            _ => Some(TopicKey::Global),
        }
    }

    pub async fn broadcast_event(&self, event: SseEvent) {
        self.broadcast_event_from(event, None).await;
    }

    /// Publish an event to its routing topic, tagged with the source
    /// client ID for echo suppression. Returns immediately — delivery
    /// is fire-and-forget through the topic's broadcast sender.
    pub async fn broadcast_event_from(&self, event: SseEvent, source_client_id: Option<String>) {
        let Some(key) = Self::topic_for(&event) else {
            tracing::error!(
                "SSE: refusing to publish a topic-less event via the inferred-topic \
                 path (SyncActions must go through broadcast_event_to with its \
                 workspace topic); dropping"
            );
            return;
        };
        self.publish(key, event, source_client_id);
    }

    /// Publish an event to an explicit topic, bypassing topic
    /// inference. Used by the sync outbox, which routes each
    /// `SyncActions` batch to the workspace topic of the rows it
    /// carries — a discriminant the event payload doesn't encode.
    pub async fn broadcast_event_to(&self, key: TopicKey, event: SseEvent) {
        self.publish(key, event, None);
    }

    fn publish(&self, key: TopicKey, event: SseEvent, source_client_id: Option<String>) {
        let id = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let env = Envelope {
            id,
            event,
            source_client_id,
        };
        let topic = self.topic(key);
        topic.record(&env);
        match topic.sender.send(env) {
            Ok(receiver_count) => {
                #[cfg(debug_assertions)]
                tracing::debug!("SSE: Event sent to {} receivers", receiver_count);
            }
            Err(_) => {
                #[cfg(debug_assertions)]
                tracing::debug!("SSE: Event recorded with no live receivers");
            }
        }
    }

    pub fn add_client(&self, client_id: String, user_id: String) {
        let mut clients = self.clients.lock().unwrap();
        clients.insert(
            client_id.clone(),
            ClientInfo {
                user_id: user_id.clone(),
                connected_at: Instant::now(),
                last_ping: Instant::now(),
            },
        );
        #[cfg(debug_assertions)]
        tracing::info!(
            "SSE: Client {} connected for user {} ({} total)",
            client_id,
            user_id,
            clients.len()
        );
    }

    pub fn remove_client(&self, client_id: &str) {
        let mut clients = self.clients.lock().unwrap();
        if clients.remove(client_id).is_some() {
            #[cfg(debug_assertions)]
            tracing::info!(
                "SSE: Client {} disconnected ({} remaining)",
                client_id,
                clients.len()
            );
        }
    }

    pub fn get_client_count(&self) -> usize {
        self.clients.lock().unwrap().len()
    }

    /// Subscribe to the Global topic — a live broadcast receiver scoped
    /// to that one topic. Used by integration tests to observe
    /// cross-resource broadcasts (e.g. asserting a merge emits its SSE
    /// events) without standing up a full SSE client.
    pub fn subscribe_global(&self) -> broadcast::Receiver<Envelope> {
        self.topic(TopicKey::Global).sender.subscribe()
    }

    /// Subscribe to a set of topics. Returns one live receiver per
    /// topic plus the replay batch (events newer than `last_event_id`
    /// across all subscribed topics, sorted by id). The caller is
    /// expected to drain the replay batch before tailing the live
    /// receivers.
    fn subscribe(
        &self,
        topics: &[TopicKey],
        last_event_id: Option<u64>,
    ) -> (Vec<broadcast::Receiver<Envelope>>, Vec<Envelope>) {
        let mut receivers = Vec::with_capacity(topics.len());
        let mut replay: Vec<Envelope> = Vec::new();
        for key in topics {
            let topic = self.topic(key.clone());
            receivers.push(topic.sender.subscribe());
            if let Some(last_id) = last_event_id {
                replay.extend(topic.replay_after(last_id));
            }
        }
        replay.sort_by_key(|e| e.id);
        (receivers, replay)
    }
}

/// Streaming body for an SSE connection. Drains `replay_queue` first
/// (events the client missed during a reconnect gap), then merges the
/// per-topic broadcast receivers into a single stream and interleaves
/// heartbeat ticks. Disconnects cleanly when any topic stream ends or
/// reports lag — the frontend reconnects with `Last-Event-ID` and the
/// server fills the gap from each topic's ring.
pub struct SseStream {
    event_streams: futures::stream::SelectAll<BroadcastStream<Envelope>>,
    replay_queue: VecDeque<Envelope>,
    /// Highest envelope id covered by the replay queue. Live events
    /// with `id <= replay_max_id` are dropped to avoid double-delivery
    /// when an event lands between subscribing to the topic and
    /// snapshotting the ring (the broadcast receiver and the ring both
    /// observe it, otherwise the client sees the same envelope twice).
    replay_max_id: u64,
    heartbeat_interval: tokio::time::Interval,
    client_id: String,
    state: web::Data<SseState>,
    /// Pool for the per-subscriber visibility filter on the live
    /// `SyncActions` stream (see `filter_sync_actions_frame`).
    pool: web::Data<crate::db::Pool>,
    /// Viewer visibility identity, captured at connect. The live stream
    /// drops rows this viewer can't see (documentation for everyone;
    /// the ticket family for restricted members), mirroring the REST
    /// delta/bootstrap paths so the live push doesn't leak. Resolved
    /// once at connect, matching the structural-at-connect topic-auth
    /// model.
    viewer: crate::sync::visibility::SyncViewer,
    /// In-flight filter for a `SyncActions` batch that needs per-viewer
    /// visibility filtering. Held across polls because the visibility
    /// check is a blocking DB call run off-thread via `web::block`.
    pending: Option<Pin<Box<dyn Future<Output = String> + Send>>>,
}

impl SseStream {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        receivers: Vec<broadcast::Receiver<Envelope>>,
        replay: Vec<Envelope>,
        client_id: String,
        state: web::Data<SseState>,
        pool: web::Data<crate::db::Pool>,
        viewer: crate::sync::visibility::SyncViewer,
    ) -> Self {
        // 15-second heartbeat. EventSource auto-reconnects when the
        // read side dies, so this is mostly a NAT/proxy keepalive
        // rather than a liveness probe.
        let mut heartbeat_interval = interval(Duration::from_secs(15));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let event_streams =
            futures::stream::select_all(receivers.into_iter().map(BroadcastStream::new));

        let replay_max_id = replay.iter().map(|e| e.id).max().unwrap_or(0);

        Self {
            event_streams,
            replay_queue: replay.into(),
            replay_max_id,
            heartbeat_interval,
            client_id,
            state,
            pool,
            viewer,
            pending: None,
        }
    }
}

/// Does this `SyncActions` envelope contain any row that needs a
/// per-viewer visibility check (documentation for everyone; the ticket
/// family for restricted viewers)? Cheap, I/O-free gate so the common
/// case (staff, or a batch of only reference/ungated rows) frames
/// immediately without the off-thread DB hop.
fn batch_needs_filtering(env: &Envelope, viewer: &crate::sync::visibility::SyncViewer) -> bool {
    let SseEvent::SyncActions { actions, .. } = &env.event else {
        return false;
    };
    let Some(rows) = actions.as_array() else {
        return false;
    };
    rows.iter().any(|row| {
        row.get("aggregate")
            .and_then(|v| v.as_str())
            .is_some_and(|wire| crate::sync::visibility::wire_aggregate_is_gated(wire, viewer))
    })
}

/// Lower one serialized-`ActionRow` JSON row into the visibility layer's
/// `ActionView`.
fn json_row_to_view(row: &serde_json::Value) -> crate::sync::visibility::ActionView {
    let aggregate = row
        .get("aggregate")
        .cloned()
        .and_then(|v| serde_json::from_value::<crate::models::SyncAggregate>(v).ok());
    let data = row.get("data");
    crate::sync::visibility::ActionView {
        aggregate,
        is_delete: row.get("op").and_then(|v| v.as_str()) == Some("D"),
        aggregate_id: row
            .get("aggregate_id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok()),
        ticket_id: data
            .and_then(|d| d.get("ticket_id"))
            .and_then(|v| v.as_i64())
            .map(|n| n as i32),
        is_internal: data
            .and_then(|d| d.get("is_internal"))
            .and_then(|v| v.as_bool()),
        comment_id: data
            .and_then(|d| d.get("comment_id"))
            .and_then(|v| v.as_i64())
            .map(|n| n as i32),
    }
}

/// Re-frame a `SyncActions` envelope with the rows this viewer may not
/// see removed, via the shared `sync::visibility::filter_actions` (the
/// same brain the REST delta/bootstrap paths use). Documentation rows
/// are dropped for everyone who can't see them; ticket-family rows are
/// dropped for restricted members. The lookup runs off-thread via
/// `web::block`; on failure it fails closed (drops every gated family,
/// keeps reference data). `last_sync_id` is preserved so the client's
/// cursor still advances past the (filtered) batch.
async fn filter_sync_actions_frame(
    pool: web::Data<crate::db::Pool>,
    viewer: crate::sync::visibility::SyncViewer,
    env: Envelope,
) -> String {
    let rows: Vec<serde_json::Value> = match &env.event {
        SseEvent::SyncActions { actions, .. } => match actions.as_array() {
            Some(a) => a.clone(),
            None => return frame_envelope(&env),
        },
        // Unreachable: the caller only routes SyncActions envelopes here.
        _ => return frame_envelope(&env),
    };

    let rows_for_block = rows.clone();
    let mask = match web::block(move || {
        let mut conn = pool.get().map_err(|_| ())?;
        Ok::<Vec<bool>, ()>(crate::sync::visibility::filter_actions(
            &mut conn,
            &viewer,
            &rows_for_block,
            json_row_to_view,
        ))
    })
    .await
    {
        Ok(Ok(m)) => m,
        _ => {
            tracing::error!("SSE sync visibility filter failed; failing closed");
            crate::sync::visibility::fail_closed_mask(&viewer, &rows, json_row_to_view)
        }
    };

    let Envelope {
        id,
        event,
        source_client_id,
    } = env;
    let SseEvent::SyncActions {
        last_xid8,
        last_sync_id,
        timestamp,
        ..
    } = event
    else {
        return String::new();
    };
    let kept: Vec<serde_json::Value> = rows
        .into_iter()
        .zip(mask)
        .filter_map(|(row, keep)| keep.then_some(row))
        .collect();

    frame_envelope(&Envelope {
        id,
        event: SseEvent::SyncActions {
            actions: serde_json::Value::Array(kept),
            last_xid8,
            last_sync_id,
            timestamp,
        },
        source_client_id,
    })
}

fn frame_envelope(env: &Envelope) -> String {
    let event_type = event_type_str(&env.event);
    let event_data = if env.source_client_id.is_some() {
        let mut value = serde_json::to_value(&env.event).unwrap_or_default();
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert(
                "source_client_id".to_string(),
                serde_json::Value::String(env.source_client_id.clone().unwrap_or_default()),
            );
        }
        serde_json::to_string(&value).unwrap_or_default()
    } else {
        serde_json::to_string(&env.event).unwrap_or_default()
    };
    format!(
        "id: {}\nevent: {}\ndata: {}\n\n",
        env.id, event_type, event_data
    )
}

impl Stream for SseStream {
    type Item = Result<actix_web::web::Bytes, actix_web::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let client_id = this.client_id.clone();

        loop {
            // 1. Drive an in-flight per-subscriber documentation filter,
            //    if one is running. It produces the already-framed bytes.
            if let Some(fut) = this.pending.as_mut() {
                match fut.as_mut().poll(cx) {
                    Poll::Ready(frame) => {
                        this.pending = None;
                        return Poll::Ready(Some(Ok(actix_web::web::Bytes::from(frame))));
                    }
                    Poll::Pending => return Poll::Pending,
                }
            }

            // 2. Next envelope: replayed backlog (reconnect gap) first,
            //    then live. Live envelopes whose id already appeared in
            //    the replay queue are skipped to avoid double-delivery.
            let env = if let Some(env) = this.replay_queue.pop_front() {
                env
            } else {
                match Pin::new(&mut this.event_streams).poll_next(cx) {
                    Poll::Ready(Some(Ok(env))) => {
                        if env.id <= this.replay_max_id {
                            continue;
                        }
                        env
                    }
                    Poll::Ready(Some(Err(
                        tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(count),
                    ))) => {
                        tracing::warn!(
                            "SSE: Client {} lagged by {} events, closing connection",
                            client_id,
                            count
                        );
                        return Poll::Ready(None);
                    }
                    Poll::Ready(None) => {
                        tracing::info!("SSE: Channel closed for client {}", client_id);
                        return Poll::Ready(None);
                    }
                    Poll::Pending => {
                        if this.heartbeat_interval.poll_tick(cx).is_ready() {
                            // Heartbeat is a sentinel — no `id:` so it
                            // doesn't advance the Last-Event-ID cursor.
                            let sse_data = "event: heartbeat\ndata: {}\n\n";
                            return Poll::Ready(Some(Ok(actix_web::web::Bytes::from(sse_data))));
                        }
                        return Poll::Pending;
                    }
                }
            };

            // 3. A SyncActions batch with rows this viewer needs
            //    visibility-checked is filtered per subscriber off-thread;
            //    everything else frames immediately. Looping back drives
            //    the future.
            if batch_needs_filtering(&env, &this.viewer) {
                this.pending = Some(Box::pin(filter_sync_actions_frame(
                    this.pool.clone(),
                    this.viewer,
                    env,
                )));
            } else {
                return Poll::Ready(Some(Ok(actix_web::web::Bytes::from(frame_envelope(&env)))));
            }
        }
    }
}

impl Drop for SseStream {
    fn drop(&mut self) {
        self.state.remove_client(&self.client_id);
    }
}

/// Parse the `Last-Event-ID` header into a u64 sequence. Browsers
/// re-send whatever the server last emitted on `id:`; if the value
/// fails to parse (alien client, manual curl) we treat it as a fresh
/// connection.
fn last_event_id_from_request(req: &HttpRequest) -> Option<u64> {
    req.headers()
        .get("Last-Event-ID")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

/// SSE routes mounted inside the authenticated `/api` scope in main.rs. The
/// event stream itself is a separate top-level service (see main.rs); this only
/// mints the short-lived connection token. The collab token lives in the
/// `/api/collaboration` scope so it isn't shadowed by this `/api` scope.
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/events/token", web::post().to(get_sse_token));
}

// SSE endpoint for ticket updates

pub async fn sse_events_stream(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    state: web::Data<SseState>,
    query: web::Query<SseEventsQuery>,
) -> ActixResult<HttpResponse> {
    // Get database connection
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            return Ok(errors::internal("Database connection error"));
        }
    };
    // Validate the SSE token first; the selected workspace is bound into it
    // (EventSource can't send the selection header), so the claims drive the
    // workspace resolution below.
    let token = match query.sse_token.as_ref() {
        Some(t) => t.as_str(),
        None => {
            return Ok(errors::unauthorized("Missing SSE token"));
        }
    };
    use crate::utils::jwt::JwtUtils;
    let (user_info, user) = match JwtUtils::validate_token_with_user_check(token, &mut conn).await {
        Ok((claims, user)) => (claims, user),
        Err(e) => {
            return Ok(e.into());
        }
    };

    // Only a purpose-minted SSE connection token opens this stream, mirroring
    // the collab WebSocket's `scope == "collab"` gate. A portal-scoped token, a
    // write-capable collab token, or a general API token is refused here — the
    // scope split is what keeps a portal principal (or any non-agent token) off
    // the agent event feed even when it is a valid member token.
    if user_info.scope != "sse" {
        return Ok(errors::forbidden("Token not valid for the event stream"));
    }

    // Resolve the stream's workspace: the token's bound selection (Model C)
    // wins, else the Host-derived context the middleware put on this request.
    use actix_web::HttpMessage as _;
    let workspace_id = match user_info.workspace_uuid {
        Some(ws_uuid) => match crate::repository::workspaces::find_by_uuid(&mut conn, ws_uuid) {
            Ok(Some(ws)) => Some(ws.id),
            // Unknown workspace collapses into the same denial a non-member
            // gets below, so workspace existence does not leak.
            Ok(None) => return Ok(errors::forbidden("Not a member of this workspace")),
            Err(e) => {
                tracing::error!(error = ?e, "SSE token workspace resolution failed");
                return Ok(errors::internal("Workspace resolution failed"));
            }
        },
        None => req
            .extensions()
            .get::<crate::extractors::WorkspaceContext>()
            .map(|w| w.workspace_id),
    };

    // Pin + membership-gate the resolved workspace so the caller's visibility
    // (their role + per-ticket checks) resolves under RLS, and they can't
    // subscribe to another tenant's live SyncActions stream. The pool clears
    // app.workspace_id on checkout, so without the pin an admin is downgraded
    // to member visibility. SSE bypasses the cookie/dual-auth middleware, so
    // the gate that middleware runs for REST is invoked explicitly here.
    //
    // The gate runs immediately after the pin with no tenant-content read in
    // between, so pinning a client-selected workspace cannot leak. Keep the
    // per-topic visibility reads (parse_topics_authorized below) after this.
    if let Some(workspace_id) = workspace_id {
        crate::handlers::helpers::pin_workspace(&mut conn, workspace_id);
        crate::middleware::cookie_auth::require_workspace_membership(
            &mut conn,
            workspace_id,
            user.uuid,
        )?;
    } else if crate::middleware::workspace_context::selection_resolution_enabled() {
        // Under Model C selection a stream MUST resolve (and be gated on) a
        // workspace; an unresolved one is a misrouted or forged connection, not
        // a legitimate apex stream, so fail closed rather than serve the
        // topic-contained user+global fallback below. (Self-hosted always
        // resolves the bootstrap workspace, so this branch is a hosted apex
        // stream only.)
        return Ok(errors::forbidden("Not a member of this workspace"));
    }

    // Resolve subscription set. The client may declare interest via
    // `?topics=user,global,ticket-42` (comma-separated). When absent,
    // `user` + `global` are subscribed for back-compat. Unknown
    // tokens are ignored. `user` is rebound to the authenticated
    // caller's uuid. `ticket-<id>` is gated by
    // ticket_visibility::can_view_ticket so unauthorised ticket
    // subscriptions are silently dropped (no existence leak).
    let mut topics = parse_topics_authorized(query.topics.as_deref(), &user_info.sub, &mut conn);

    // The live SyncActions feed rides the workspace topic, and the
    // subscription is server-assigned, never client-selected: exactly
    // the workspace this connection pinned and membership-gated above.
    // No pinned workspace, no sync feed — the same fail-closed shape
    // as the RLS policies the REST reads sit behind.
    if let Some(workspace_id) = workspace_id {
        topics.push(TopicKey::Workspace(workspace_id));
    }

    let last_event_id = last_event_id_from_request(&req);
    let (receivers, replay) = state.subscribe(&topics, last_event_id);

    if let Some(last_id) = last_event_id {
        tracing::debug!(
            "SSE: Reconnect for user {} replaying {} events after id {}",
            user_info.sub,
            replay.len(),
            last_id
        );
    }

    // Viewer visibility identity for the per-subscriber filter on the
    // live SyncActions stream. Resolved once at connect (structural, like
    // topic auth); cached for the connection's lifetime.
    let viewer = crate::sync::visibility::SyncViewer::resolve(&mut conn, &user);

    // Generate client ID and create stream
    let client_id = Uuid::now_v7().to_string();
    state.add_client(client_id.clone(), user_info.sub.clone());
    let stream = SseStream::new(
        receivers,
        replay,
        client_id.clone(),
        state.clone(),
        pool.clone(),
        viewer,
    );

    // Build initial "connected" event so the client knows its own ID
    let connected_data = json!({ "client_id": client_id }).to_string();
    let initial_event = format!("event: connected\ndata: {connected_data}\n\n");

    // Chain the initial event with the ongoing stream
    let initial = futures::stream::once(async move {
        Ok::<_, actix_web::Error>(actix_web::web::Bytes::from(initial_event))
    });
    let full_stream = initial.chain(stream);

    // Return SSE response with optimized headers
    Ok(HttpResponse::Ok()
        .append_header(("Content-Type", "text/event-stream"))
        .append_header(("Cache-Control", "no-cache"))
        .append_header(("Connection", "keep-alive"))
        .append_header(("X-Accel-Buffering", "no")) // Disable nginx buffering
        .streaming(full_stream))
}

#[derive(Deserialize)]
pub struct SseEventsQuery {
    sse_token: Option<String>,
    /// Optional comma-separated list of topic tokens. Recognised
    /// tokens: `user` (the caller's personal topic), `global` (the
    /// shared cross-resource topic). When missing or empty, both
    /// topics are subscribed, preserving back-compat with clients
    /// that pre-date the topic model.
    topics: Option<String>,
}

/// Parse the `topics` query parameter into a list of `TopicKey`s
/// the caller is allowed to subscribe to. Authorisation happens
/// here so the filter is structural — once a subscriber has the
/// receiver, every event on that topic is theirs to consume.
///
/// Recognised tokens:
///
/// * `global`          — the shared cross-resource topic.
/// * `user`            — the caller's personal topic. Always
///                       rebound to the caller's uuid; we never
///                       let one client subscribe to another
///                       user's personal channel.
/// * `ticket-<i32>`    — ticket-scoped presence events. Allowed
///                       only when `ticket_visibility::can_view_ticket`
///                       returns true for the caller. Tokens for
///                       tickets the caller can't see (or that
///                       don't exist) are silently dropped so the
///                       subscription doesn't leak existence.
///
/// Unknown / malformed / unauthorised tokens are dropped rather
/// than rejected so a newer client can still connect to an older
/// server.
fn parse_topics_authorized(
    raw: Option<&str>,
    caller_uuid: &str,
    conn: &mut crate::db::DbConnection,
) -> Vec<TopicKey> {
    use crate::repository::ticket_visibility::{can_view_ticket, VisibilityContext};

    let trimmed = raw.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        return vec![TopicKey::Global, TopicKey::User(caller_uuid.to_string())];
    }

    // Build the visibility context once if we need it for any
    // ticket-<id> token.
    let mut cached_vis: Option<Option<VisibilityContext>> = None;

    let mut out = Vec::new();
    let mut seen_global = false;
    let mut seen_user = false;
    let mut seen_tickets = std::collections::HashSet::new();

    for token in trimmed.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if token == "global" {
            if !seen_global {
                out.push(TopicKey::Global);
                seen_global = true;
            }
            continue;
        }
        if token == "user" {
            if !seen_user {
                out.push(TopicKey::User(caller_uuid.to_string()));
                seen_user = true;
            }
            continue;
        }
        if let Some(rest) = token.strip_prefix("ticket-") {
            let Ok(ticket_id) = rest.parse::<i32>() else {
                continue;
            };
            if !seen_tickets.insert(ticket_id) {
                continue;
            }
            // Lazily build the VisibilityContext from the caller's
            // claims string. A malformed sub string (shouldn't
            // happen post-auth, but treat defensively) drops every
            // ticket subscription.
            let vis = cached_vis.get_or_insert_with(|| {
                // Resolve the caller's visibility from their DB row
                // (platform role + bootstrap-workspace role). A
                // malformed sub string (shouldn't happen post-auth,
                // but treat defensively) drops every ticket
                // subscription.
                let user_uuid = uuid::Uuid::parse_str(caller_uuid).ok()?;
                lookup_vis_for(conn, user_uuid)
            });
            let Some(vis_ctx) = vis.as_ref() else {
                continue;
            };
            if can_view_ticket(conn, vis_ctx, ticket_id).unwrap_or(false) {
                out.push(TopicKey::Ticket(ticket_id));
            }
            // else: silently drop — denying with a status leaks ticket
            // existence to the caller.
        }
        // Other tokens silently dropped (back-compat).
    }
    if out.is_empty() {
        return vec![TopicKey::Global, TopicKey::User(caller_uuid.to_string())];
    }
    out
}

fn lookup_vis_for(
    conn: &mut crate::db::DbConnection,
    user_uuid: uuid::Uuid,
) -> Option<crate::repository::ticket_visibility::VisibilityContext> {
    let user = crate::repository::users::get_user_by_uuid(&user_uuid, conn).ok()?;
    let workspace_role = crate::repository::user_helpers::workspace_role(conn, user_uuid);
    Some(
        crate::repository::ticket_visibility::VisibilityContext::new(
            user_uuid,
            crate::models::PlatformRole::from_db(&user.platform_role),
            workspace_role,
        ),
    )
}

// SSE status endpoint
pub async fn sse_status(state: web::Data<SseState>) -> impl actix_web::Responder {
    let client_count = state.get_client_count();

    HttpResponse::Ok().json(json!({
        "connected_clients": client_count,
        "status": "running"
    }))
}

// Secure endpoint to get SSE token
pub async fn get_sse_token(
    req: actix_web::HttpRequest,
    pool: web::Data<crate::db::Pool>,
) -> impl actix_web::Responder {
    use actix_web::HttpMessage;

    let _conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            return errors::internal("Database connection error");
        }
    };

    // Extract claims from request extensions (set by cookie_auth_middleware)
    let user_info = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return errors::unauthorized("Authentication required");
        }
    };

    // Bind the request's resolved workspace into the SSE token. The auth
    // middleware has already resolved it (selection header or Host) and
    // membership-gated it, so the stream can authorize against the same
    // workspace without the selection header EventSource can't send. `None`
    // when no workspace resolved (apex / self-hosted bootstrap is still set).
    let workspace_uuid = req
        .extensions()
        .get::<crate::extractors::WorkspaceContext>()
        .map(|w| w.workspace_uuid);

    // Generate a short-lived SSE token (1 hour)
    use crate::utils::jwt::JwtUtils;
    let sse_token = match JwtUtils::create_sse_token(
        &user_info.sub,
        &user_info.platform_role,
        workspace_uuid,
    ) {
        Ok(token) => token,
        Err(_) => {
            return errors::internal("Failed to create SSE token");
        }
    };

    HttpResponse::Ok().json(json!({
        "sse_token": sse_token,
        "expires_in": crate::utils::jwt::CONNECTION_TOKEN_TTL_SECS,
        "user_id": user_info.sub
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sync_actions_env(actions: serde_json::Value) -> Envelope {
        Envelope {
            id: 1,
            event: SseEvent::SyncActions {
                actions,
                last_xid8: 1,
                last_sync_id: 1,
                timestamp: chrono::Utc::now(),
            },
            source_client_id: None,
        }
    }

    fn viewer(sees_all: bool) -> crate::sync::visibility::SyncViewer {
        use crate::models::{PlatformRole, WorkspaceRole};
        use crate::repository::ticket_visibility::VisibilityContext;
        let role = if sees_all {
            WorkspaceRole::Agent
        } else {
            WorkspaceRole::Member
        };
        crate::sync::visibility::SyncViewer {
            ctx: VisibilityContext::new(
                uuid::Uuid::nil(),
                PlatformRole::from_db("user"),
                Some(role),
            ),
            is_doc_admin: false,
        }
    }

    #[test]
    fn needs_filtering_documentation_gates_everyone() {
        let env = sync_actions_env(json!([
            { "aggregate": "user", "aggregate_id": "x" },
            { "aggregate": "documentation_page", "aggregate_id": "7" },
        ]));
        // Docs need a visibility check even for staff.
        assert!(batch_needs_filtering(&env, &viewer(true)));
        assert!(batch_needs_filtering(&env, &viewer(false)));
    }

    #[test]
    fn needs_filtering_ticket_family_only_for_restricted() {
        let env = sync_actions_env(json!([
            { "aggregate": "ticket", "aggregate_id": "5" },
            { "aggregate": "comment", "aggregate_id": "8" },
        ]));
        // Staff see all tickets -> no filter; members must be filtered.
        assert!(!batch_needs_filtering(&env, &viewer(true)));
        assert!(batch_needs_filtering(&env, &viewer(false)));
    }

    #[test]
    fn needs_filtering_reference_only_skips() {
        let env = sync_actions_env(json!([
            { "aggregate": "user", "aggregate_id": "x" },
            { "aggregate": "asset", "aggregate_id": "1" },
        ]));
        assert!(!batch_needs_filtering(&env, &viewer(true)));
        assert!(!batch_needs_filtering(&env, &viewer(false)));
    }

    #[test]
    fn sync_actions_has_no_inferred_topic() {
        let Envelope { event, .. } = sync_actions_env(json!([]));
        // Routing SyncActions by inference would put tenant rows on
        // the shared Global topic; topic_for must refuse it.
        assert_eq!(SseState::topic_for(&event), None);
    }

    #[tokio::test]
    async fn sync_actions_route_to_the_workspace_topic_only() {
        let state = SseState::new();
        let mut global_rx = state.subscribe_global();
        let (mut ws_rxs, _) = state.subscribe(&[TopicKey::Workspace(2)], None);
        let event = || SseEvent::SyncActions {
            actions: json!([]),
            last_xid8: 1,
            last_sync_id: 1,
            timestamp: chrono::Utc::now(),
        };

        // The inferred-topic path refuses tenant rows outright…
        state.broadcast_event(event()).await;
        // …the explicit per-workspace publish is the only delivery path.
        state
            .broadcast_event_to(TopicKey::Workspace(2), event())
            .await;

        let got = ws_rxs[0]
            .try_recv()
            .expect("workspace subscriber receives the batch");
        assert!(matches!(got.event, SseEvent::SyncActions { .. }));
        assert!(
            global_rx.try_recv().is_err(),
            "the Global topic must never carry SyncActions"
        );
    }

    #[test]
    fn json_row_to_view_extracts_fields() {
        let row = json!({
            "aggregate": "comment",
            "op": "I",
            "aggregate_id": "42",
            "data": { "ticket_id": 7, "is_internal": true, "comment_id": 9 }
        });
        let v = json_row_to_view(&row);
        assert_eq!(v.aggregate, Some(crate::models::SyncAggregate::Comment));
        assert!(!v.is_delete);
        assert_eq!(v.aggregate_id, Some(42));
        assert_eq!(v.ticket_id, Some(7));
        assert_eq!(v.is_internal, Some(true));
        assert_eq!(v.comment_id, Some(9));

        let del = json!({ "aggregate": "attachment", "op": "D", "aggregate_id": "3", "data": { "id": 3 } });
        let dv = json_row_to_view(&del);
        assert!(dv.is_delete);
        assert_eq!(dv.comment_id, None);
    }
}
