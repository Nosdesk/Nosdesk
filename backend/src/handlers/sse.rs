use actix_web::{web, HttpRequest, HttpResponse, Result as ActixResult};

use crate::handlers::errors;
use dashmap::DashMap;
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
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
    TicketUpdated {
        ticket_id: i32,
        field: String,
        value: serde_json::Value,
        updated_by: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TicketCreated {
        ticket_id: i32,
        ticket: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TicketDeleted {
        ticket_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// A merge committed. Open viewers of a source ticket show the
    /// "merged into #N" banner; the destination's viewers refetch to
    /// pick up the marker comment and the merged-in sidebar.
    TicketMerged {
        target_ticket_id: i32,
        source_ticket_ids: Vec<i32>,
        actor_uuid: String,
        merge_event_id: i64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    CommentAdded {
        ticket_id: i32,
        comment: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    CommentDeleted {
        ticket_id: i32,
        comment_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    AttachmentAdded {
        ticket_id: i32,
        comment_id: i32,
        attachment: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    AttachmentDeleted {
        ticket_id: i32,
        comment_id: i32,
        attachment_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    AssetLinked {
        ticket_id: i32,
        device_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    AssetUnlinked {
        ticket_id: i32,
        device_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    AssetCreated {
        device_id: i32,
        device: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    AssetUpdated {
        device_id: i32,
        field: String,
        value: serde_json::Value,
        updated_by: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    AssetDeleted {
        device_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Emitted after a usage decrement that drops a stock-tracked
    /// asset's quantity to at-or-below its configured
    /// `low_stock_threshold`, having been above it before the
    /// decrement. Carries the new quantity + threshold + unit so
    /// the frontend can render a toast without re-fetching.
    AssetLowStock {
        device_id: i32,
        device_name: String,
        quantity: String,
        threshold: String,
        unit: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Audit-count event. Distinct from AssetUsageRecorded
    /// because audits replace the asset quantity rather than
    /// adjust it; the payload carries both the new (counted)
    /// quantity and the previous so the frontend can render
    /// "Counted 42, was 50 (-8)" without re-fetching.
    AssetAuditRecorded {
        audit_id: i64,
        asset_id: i32,
        asset_name: String,
        counted_quantity: String,
        previous_quantity: String,
        delta: String,
        unit: String,
        notes: Option<String>,
        recorded_at: chrono::DateTime<chrono::Utc>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Append-only ledger row that just landed. Lets the usage
    /// history panels on both the asset detail and the ticket
    /// detail refresh reactively without a refetch. The payload
    /// is the full ledger row plus the asset's display name so
    /// the frontend can render the line without a join.
    AssetUsageRecorded {
        usage_id: i64,
        asset_id: i32,
        asset_name: String,
        ticket_id: Option<i32>,
        quantity_used: String,
        unit: String,
        event_kind: String,
        notes: Option<String>,
        recorded_at: chrono::DateTime<chrono::Utc>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    ProjectAssigned {
        ticket_id: i32,
        project_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    ProjectUnassigned {
        ticket_id: i32,
        project_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TicketLinked {
        ticket_id: i32,
        linked_ticket_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TicketUnlinked {
        ticket_id: i32,
        linked_ticket_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    DocumentationCreated {
        document_id: i32,
        document: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    DocumentationUpdated {
        document_id: i32,
        field: String,
        value: serde_json::Value,
        updated_by: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    CollectionUpdated {
        collection_id: i32,
        field: String,
        value: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    KnowledgeGapDetected {
        gap_id: i64,
        signal_type: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    KnowledgeGapResolved {
        gap_id: i64,
        resolved_page_id: Option<i32>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
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
    UserUpdated {
        user_uuid: String,
        field: String,
        value: serde_json::Value,
        updated_by: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    UserCreated {
        user_uuid: String,
        user: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    UserDeleted {
        user_uuid: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// User row stamped with `deleted_at`. Active surfaces drop
    /// them; the row stays in the table for the retention window.
    UserSoftDeleted {
        user_uuid: String,
        deleted_at: chrono::NaiveDateTime,
        purge_at: chrono::NaiveDateTime,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Admin restored a soft-deleted user. Active surfaces start
    /// rendering them again on the next sync delta.
    UserRestored {
        user_uuid: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Retention worker (or admin permanent-delete) hard-deleted
    /// the row. Frontends drop the row from caches; this is the
    /// "really gone" signal, distinct from soft-delete.
    UserPurged {
        user_uuid: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    Heartbeat {
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Notification received (targeted to specific user)
    NotificationReceived {
        recipient_uuid: String,
        notification: serde_json::Value,
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
        last_sync_id: i64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// SLA breach fired by the scheduled detection sweep. The pill
    /// repaint flows through `ticket.sla_updated` sync_actions
    /// already (see `services::scheduled_jobs::detect_sla_breaches`);
    /// this event is the webhook-fanout channel so subscribers
    /// listening on the global SSE topic can pick up
    /// `ticket.sla_breached` deliveries. Carries the timer kind so
    /// downstream consumers can route response vs resolution breaches
    /// separately without resolving the ticket payload themselves.
    SlaBreached {
        ticket_id: i32,
        ticket_title: String,
        timer: &'static str,
        target_at: chrono::DateTime<chrono::Utc>,
        breached_at: chrono::DateTime<chrono::Utc>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
}

fn event_type_str(event: &SseEvent) -> &'static str {
    match event {
        SseEvent::TicketUpdated { .. } => "ticket-updated",
        SseEvent::TicketCreated { .. } => "ticket-created",
        SseEvent::TicketDeleted { .. } => "ticket-deleted",
        SseEvent::TicketMerged { .. } => "ticket-merged",
        SseEvent::CommentAdded { .. } => "comment-added",
        SseEvent::CommentDeleted { .. } => "comment-deleted",
        SseEvent::AttachmentAdded { .. } => "attachment-added",
        SseEvent::AttachmentDeleted { .. } => "attachment-deleted",
        SseEvent::AssetLinked { .. } => "asset-linked",
        SseEvent::AssetUnlinked { .. } => "asset-unlinked",
        SseEvent::AssetCreated { .. } => "asset-created",
        SseEvent::AssetUpdated { .. } => "asset-updated",
        SseEvent::AssetDeleted { .. } => "asset-deleted",
        SseEvent::AssetLowStock { .. } => "asset-low-stock",
        SseEvent::AssetUsageRecorded { .. } => "asset-usage-recorded",
        SseEvent::AssetAuditRecorded { .. } => "asset-audit-recorded",
        SseEvent::ProjectAssigned { .. } => "project-assigned",
        SseEvent::ProjectUnassigned { .. } => "project-unassigned",
        SseEvent::TicketLinked { .. } => "ticket-linked",
        SseEvent::TicketUnlinked { .. } => "ticket-unlinked",
        SseEvent::DocumentationCreated { .. } => "documentation-created",
        SseEvent::DocumentationUpdated { .. } => "documentation-updated",
        SseEvent::CollectionUpdated { .. } => "collection-updated",
        SseEvent::KnowledgeGapDetected { .. } => "knowledge-gap-detected",
        SseEvent::KnowledgeGapResolved { .. } => "knowledge-gap-resolved",
        SseEvent::ViewersChanged { .. } => "viewers-changed",
        SseEvent::TicketFieldPreviewed { .. } => "ticket-field-previewed",
        SseEvent::UserUpdated { .. } => "user-updated",
        SseEvent::UserCreated { .. } => "user-created",
        SseEvent::UserDeleted { .. } => "user-deleted",
        SseEvent::UserSoftDeleted { .. } => "user-soft-deleted",
        SseEvent::UserRestored { .. } => "user-restored",
        SseEvent::UserPurged { .. } => "user-purged",
        SseEvent::Heartbeat { .. } => "heartbeat",
        SseEvent::NotificationReceived { .. } => "notification-received",
        SseEvent::SyncActions { .. } => "sync-actions",
        SseEvent::SlaBreached { .. } => "ticket.sla_breached",
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
/// events published there. Phase A keeps the topology small: `Global`
/// carries every cross-resource event, `User(uuid)` carries targeted
/// notifications, and `Ticket(id)` carries presence-style events whose
/// payload is sensitive enough that only authorised subscribers may
/// receive them.
///
/// Subscription to `Ticket(id)` is gated by
/// `ticket_visibility::can_view_ticket` at connect time (see
/// `parse_topics_authorized`), so the visibility filter is structural
/// — it doesn't run per event.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TopicKey {
    Global,
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
    /// so they live on a `User` topic. Everything else fans out on
    /// `Global` for now; future phases can split this into per-resource
    /// topics for finer-grained delivery.
    fn topic_for(event: &SseEvent) -> TopicKey {
        match event {
            SseEvent::NotificationReceived { recipient_uuid, .. } => {
                TopicKey::User(recipient_uuid.clone())
            }
            SseEvent::ViewersChanged { ticket_id, .. } => TopicKey::Ticket(*ticket_id),
            SseEvent::TicketFieldPreviewed { ticket_id, .. } => TopicKey::Ticket(*ticket_id),
            _ => TopicKey::Global,
        }
    }

    pub async fn broadcast_event(&self, event: SseEvent) {
        self.broadcast_event_from(event, None).await;
    }

    /// Publish an event to its routing topic, tagged with the source
    /// client ID for echo suppression. Returns immediately — delivery
    /// is fire-and-forget through the topic's broadcast sender.
    pub async fn broadcast_event_from(&self, event: SseEvent, source_client_id: Option<String>) {
        let key = Self::topic_for(&event);
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

    /// Broadcast a notification targeted at a specific user. Routed
    /// via `User(recipient_uuid)` so other users' connections never
    /// receive the payload.
    pub async fn broadcast_notification(
        &self,
        recipient_uuid: String,
        notification: crate::services::notifications::NotificationEvent,
    ) {
        let event = SseEvent::NotificationReceived {
            recipient_uuid,
            notification: serde_json::to_value(&notification).unwrap_or_default(),
            timestamp: chrono::Utc::now(),
        };
        self.broadcast_event(event).await;
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

    /// Subscribe to the Global topic. Used by integrations like the
    /// webhook listener that need to observe every cross-resource
    /// event without seeing per-user notifications. Returns a live
    /// broadcast receiver scoped to that one topic.
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
}

impl SseStream {
    pub fn new(
        receivers: Vec<broadcast::Receiver<Envelope>>,
        replay: Vec<Envelope>,
        client_id: String,
        state: web::Data<SseState>,
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
        }
    }
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

        // Drain any replayed envelopes (events that arrived during the
        // client's reconnect gap) before pulling from the live streams.
        if let Some(env) = this.replay_queue.pop_front() {
            return Poll::Ready(Some(Ok(actix_web::web::Bytes::from(frame_envelope(&env)))));
        }

        let client_id = this.client_id.clone();

        // Drain live envelopes one at a time, skipping any whose id
        // already appeared in the replay queue. The loop exits as soon
        // as we either return an envelope or hit Pending.
        loop {
            match Pin::new(&mut this.event_streams).poll_next(cx) {
                Poll::Ready(Some(Ok(env))) => {
                    if env.id <= this.replay_max_id {
                        continue;
                    }
                    return Poll::Ready(Some(Ok(actix_web::web::Bytes::from(frame_envelope(
                        &env,
                    )))));
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
                Poll::Pending => break,
            }
        }

        if this.heartbeat_interval.poll_tick(cx).is_ready() {
            // Heartbeat is a sentinel — no `id:` so it doesn't advance
            // the client's Last-Event-ID cursor.
            let sse_data = "event: heartbeat\ndata: {}\n\n";
            return Poll::Ready(Some(Ok(actix_web::web::Bytes::from(sse_data))));
        }

        Poll::Pending
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

    // Validate SSE token
    let token = match query.sse_token.as_ref() {
        Some(t) => t.as_str(),
        None => {
            return Ok(errors::unauthorized("Missing SSE token"));
        }
    };

    // Validate the SSE token
    use crate::utils::jwt::JwtUtils;
    let (user_info, _user) = match JwtUtils::validate_token_with_user_check(token, &mut conn).await
    {
        Ok((claims, user)) => (claims, user),
        Err(e) => {
            return Ok(e.into());
        }
    };

    // Resolve subscription set. The client may declare interest via
    // `?topics=user,global,ticket-42` (comma-separated). When absent,
    // `user` + `global` are subscribed for back-compat. Unknown
    // tokens are ignored. `user` is rebound to the authenticated
    // caller's uuid. `ticket-<id>` is gated by
    // ticket_visibility::can_view_ticket so unauthorised ticket
    // subscriptions are silently dropped (no existence leak).
    let topics = parse_topics_authorized(query.topics.as_deref(), &user_info.sub, &mut conn);

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

    // Generate client ID and create stream
    let client_id = Uuid::now_v7().to_string();
    state.add_client(client_id.clone(), user_info.sub.clone());
    let stream = SseStream::new(receivers, replay, client_id.clone(), state.clone());

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
                use crate::models::Claims;
                let stub_claims = Claims {
                    sub: caller_uuid.to_string(),
                    name: String::new(),
                    email: String::new(),
                    role: String::from("user"),
                    platform_role: None,
                    scope: String::new(),
                    sid: None,
                    exp: 0,
                    iat: 0,
                };
                // VisibilityContext::from_claims only reads sub +
                // role. The stub above is enough; we re-resolve the
                // real role from the DB row that the caller of
                // parse_topics_authorized already validated.
                let user_uuid = uuid::Uuid::parse_str(caller_uuid).ok()?;
                let _ = stub_claims;
                Some(VisibilityContext {
                    user_uuid,
                    role: lookup_role_for(conn, user_uuid)?,
                })
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

fn lookup_role_for(
    conn: &mut crate::db::DbConnection,
    user_uuid: uuid::Uuid,
) -> Option<crate::models::UserRole> {
    crate::repository::users::get_user_by_uuid(&user_uuid, conn)
        .ok()
        .map(|u| {
            crate::repository::user_helpers::legacy_role_for_user(conn, u.uuid, &u.platform_role)
        })
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

    // Generate a short-lived SSE token (1 hour)
    use crate::utils::jwt::JwtUtils;
    let sse_token = match JwtUtils::create_sse_token(&user_info.sub, &user_info.role) {
        Ok(token) => token,
        Err(_) => {
            return errors::internal("Failed to create SSE token");
        }
    };

    HttpResponse::Ok().json(json!({
        "sse_token": sse_token,
        "expires_in": 3600,
        "user_id": user_info.sub
    }))
}
