use actix_web::{web, Error, HttpRequest, HttpResponse, Responder};
use actix_ws::{AggregatedMessage, CloseReason};
use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::panic;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Notify, RwLock};
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;
use yrs::sync::{Awareness, DefaultProtocol, Protocol};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, ReadTxn, StateVector, Transact, Update, WriteTxn, XmlFragment};

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::repository;
use crate::sync::actor::ActorContext as DbActor;
use crate::sync::session;

/// Workspace-pinned system actor for the Yjs WebSocket session's
/// background DB writes (snapshot saves, revision creates,
/// content updates). The workspace_id is resolved at WebSocket
/// handshake from `WorkspaceContext` (subdomain routing) and
/// threaded through `YjsWebSocket` / `DocumentState` so every
/// per-document write runs under a workspace-pinned actor via
/// `session::with_actor_context` instead of bypassing RLS.
fn yjs_session_actor(workspace_id: i32) -> DbActor {
    DbActor::system("yjs-collab").with_workspace(workspace_id)
}

/// Encode a document's full state as a Yjs v1 update. The single place
/// the on-the-wire encoding for durable saves is defined.
fn encode_doc_update(awareness: &Awareness) -> Vec<u8> {
    awareness
        .doc()
        .transact()
        .encode_state_as_update_v1(&StateVector::default())
}

/// Encode both the full v1 update and the encoded state vector in one
/// transaction. Used by the crash-recovery checkpoint write, whose table
/// stores both columns.
fn encode_doc_full(awareness: &Awareness) -> (Vec<u8>, Vec<u8>) {
    let doc = awareness.doc();
    let txn = doc.transact();
    (
        txn.encode_state_as_update_v1(&StateVector::default()),
        txn.state_vector().encode_v1(),
    )
}

/// Construct a fresh server-side Yjs document for `doc_id`, applying the
/// conventions every collaborative doc on this server shares. The single
/// place that decision lives, so cold-load and revision-restore build
/// byte-identical documents:
///
/// * GC disabled, so the server can always emit a complete state as a
///   v1 update (durable saves and revision snapshots depend on it).
/// * A deterministic 53-bit client id derived from `doc_id`: stable
///   across restarts (no state-vector churn) and inside JS's safe
///   integer range, since yrs 0.26 ClientIDs are 53-bit to match Yjs.
/// * The `"prosemirror"` root XmlFragment declared up front, per the
///   yrs guidance to define all root types during document creation.
///   Later `apply_update` calls merge stored state into this structure.
fn new_server_doc(doc_id: &str) -> Doc {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    doc_id.hash(&mut hasher);
    // Mask the 64-bit hash into 53 bits; OR with 1 to stay non-zero.
    let client_id = (hasher.finish() & ((1u64 << 53) - 1)) | 1;

    let doc = Doc::with_options(yrs::Options {
        client_id: yrs::ClientID::new(client_id),
        skip_gc: true,
        ..yrs::Options::default()
    });
    {
        let mut txn = doc.transact_mut();
        let _ = txn.get_or_insert_xml_fragment("prosemirror");
    }
    doc
}

/// Rebuild a collaborative document from a stored revision snapshot and
/// make it the live document: swap it into `app_state`, flag it for
/// persistence, and broadcast the restored state to connected clients.
///
/// yrs updates merge (a union of operations, never a delete), so a
/// revision can't be reverted in place. The idiomatic restore is to
/// rebuild a fresh doc from the revision's full-state snapshot
/// (`encode_state_as_update_v1(&StateVector::default())`, the format
/// every revision stores) and replace the live one. Already-open
/// editors merge the broadcast like any update; a hard revert there
/// relies on the client reloading after a restore.
async fn restore_revision_snapshot(
    app_state: &YjsAppState,
    doc_id: &str,
    workspace_id: i32,
    doc_type: DocumentType,
    snapshot: &[u8],
) -> Result<(), HttpResponse> {
    let update = Update::decode_v1(snapshot).map_err(|e| {
        error!(doc_id, error = ?e, "Error decoding revision snapshot");
        errors::internal("Error decoding revision")
    })?;

    let doc = new_server_doc(doc_id);
    {
        let mut txn = doc.transact_mut();
        txn.apply_update(update).map_err(|e| {
            error!(doc_id, error = ?e, "Error applying revision snapshot");
            errors::internal("Error applying revision")
        })?;
    }

    let full_state = doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default());

    app_state
        .replace_document(doc_id, doc, workspace_id, doc_type)
        .await;
    app_state.mark_document_changed(doc_id).await;

    use yrs::sync::{Message, SyncMessage};
    let restored = Message::Sync(SyncMessage::Update(full_state)).encode_v1();
    app_state.broadcast(doc_id, "", &restored).await;

    Ok(())
}

/// Persist an encoded Yjs update to its backing table. Awaitable, so it
/// serves two callers with one workspace-pinned, RLS-enforced,
/// fence-gated write path (DRY): the periodic / on-disconnect save
/// (`save_document_internal`) spawns it fire-and-forget; the
/// graceful-shutdown flush (`flush_all_dirty`) awaits it so the write
/// lands before the process exits. Errors are logged, not propagated: a
/// failed save must not abort a maintenance tick or a shutdown.
async fn write_yjs_state(
    pool: web::Data<crate::db::Pool>,
    search: Arc<crate::services::search::SearchService>,
    doc_type: DocumentType,
    content: Vec<u8>,
    workspace_id: i32,
    fence: Option<i64>,
    // True only for a save that ends an editing session which had real
    // changes (room emptied / last editor disconnected, contributors
    // non-empty). On such a save, also emit a search-only
    // `*.content_saved` sync_action — atomic with the body write — so the
    // index replicator re-indexes the collaborative body on every machine.
    // Collaborative saves are otherwise off the sync stream, so without
    // this a finished edit is only searchable on the owning machine.
    emit_saved: bool,
) {
    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            error!(?doc_type, error = ?e, "DB connection error saving Yjs state");
            return;
        }
    };
    let actor = yjs_session_actor(workspace_id);
    let result: Result<(), diesel::result::Error> = match doc_type {
        DocumentType::Ticket(ticket_id) => session::with_actor_context(&mut conn, &actor, |conn| {
            repository::update_article_yjs_state(conn, ticket_id, content, fence, Some(&search))?;
            if emit_saved {
                emit_content_saved(
                    conn,
                    crate::models::SyncAggregate::Ticket,
                    ticket_id,
                    "ticket.content_saved",
                )?;
            }
            Ok(())
        }),
        DocumentType::Documentation(page_id) => {
            session::with_actor_context(&mut conn, &actor, |conn| {
                repository::update_documentation_yjs_state(
                    conn,
                    page_id,
                    content,
                    fence,
                    Some(&search),
                )?;
                if emit_saved {
                    emit_content_saved(
                        conn,
                        crate::models::SyncAggregate::DocumentationPage,
                        page_id,
                        "documentation.content_saved",
                    )?;
                }
                Ok(())
            })
        }
        DocumentType::Collection(collection_id) => {
            // Collection descriptions aren't body-searchable, so no
            // content-saved emit.
            session::with_actor_context(&mut conn, &actor, |conn| {
                repository::documentation_collections::update_collection_description_yjs(
                    conn,
                    collection_id,
                    content,
                    fence,
                )
                .map(|_| ())
            })
        }
    };
    match result {
        Ok(()) => debug!(?doc_type, "Saved Yjs state"),
        Err(e) => error!(?doc_type, error = ?e, "Failed to save Yjs state"),
    }
}

/// Emit a search-only `*.content_saved` sync_action for a finished
/// collaborative editing session, so the search-index replicator re-indexes
/// the document body on every machine (see `services::search_replicator`).
///
/// Deliberately invisible to everything else: emitted with EMPTY groups so
/// it reaches no live SSE subscriber (editors already have the content via
/// Yjs), and the `*.content_saved` event type maps to no `WebhookEventType`
/// and no activity-feed entry, so it fires no webhook and shows nothing in
/// the UI. The replicator drains the `sync_actions` table directly,
/// independent of groups, so it still sees the row.
fn emit_content_saved(
    conn: &mut crate::db::DbConnection,
    aggregate: crate::models::SyncAggregate,
    entity_id: i32,
    event_type: &'static str,
) -> Result<(), diesel::result::Error> {
    crate::sync::emit::record(
        conn,
        crate::sync::emit::SyncEmit {
            aggregate,
            aggregate_id: entity_id.to_string(),
            op: crate::models::SyncOp::Update,
            event_type,
            data: serde_json::json!({ "id": entity_id }),
            groups: Vec::new(),
            causation_id: None,
        },
    )
    .map(|_| ())
}

/// Safely get string content from a Yjs XmlFragment
/// Returns None if the fragment contains invalid UTF-8 data (which can cause yrs to panic)
fn safe_get_fragment_string(
    fragment: &yrs::XmlFragmentRef,
    txn: &yrs::Transaction,
) -> Option<String> {
    panic::catch_unwind(panic::AssertUnwindSafe(|| fragment.get_string(txn))).ok()
}

/// Get a preview of document content for logging
fn get_content_preview(awareness: &Awareness, max_chars: usize) -> String {
    let txn = awareness.doc().transact();
    if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
        // Get children count for diagnostic purposes
        let children_count = fragment.len(&txn);
        let text_content = match safe_get_fragment_string(&fragment, &txn) {
            Some(s) => s.chars().take(max_chars).collect::<String>(),
            None => "(invalid char data)".to_string(),
        };

        // Empty text with children - log structure info
        if text_content.is_empty() && children_count > 0 {
            format!("[{children_count} children, text: '']")
        } else if text_content.is_empty() {
            "[0 children]".to_string()
        } else {
            text_content
        }
    } else {
        "(no fragment)".to_string()
    }
}

/// Log all root-level types in a Yjs document for debugging
fn log_document_root_types(awareness: &Awareness, doc_id: &str) {
    let doc = awareness.doc();
    let txn = doc.transact();

    // Get all root-level type names using root_refs iterator
    let root_names: Vec<String> = txn.root_refs().map(|(name, _)| name.to_string()).collect();

    trace!(doc_id = %doc_id, root_types = ?root_names, "Root types in document");

    // Check prosemirror fragment specifically
    if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
        // XmlFragment children count using both methods
        let children_iter: usize = fragment.children(&txn).count();
        let children_len = fragment.len(&txn);
        trace!(doc_id = %doc_id, children_iter, children_len, "prosemirror XmlFragment");

        // Try to iterate and describe children (only log count, not individual items in trace)
        let child_count = fragment.children(&txn).take(6).count();
        if child_count > 5 {
            trace!(doc_id = %doc_id, child_count = "5+", "prosemirror children");
        }

        // Try get_string
        let text = fragment.get_string(&txn);
        if text.is_empty() {
            trace!(doc_id = %doc_id, "prosemirror get_string() is empty");
        } else {
            let preview: String = text.chars().take(100).collect();
            trace!(doc_id = %doc_id, preview = %preview, "prosemirror content preview");
        }
    } else {
        trace!(doc_id = %doc_id, "prosemirror fragment not found");
    }

    // Log state vector to see client contributions
    let sv = txn.state_vector();
    trace!(doc_id = %doc_id, state_vector = ?sv, "Document state vector");
}
use crate::models::{NewArticleContent, NewArticleContentRevision};
use crate::utils::redis_yjs_cache::RedisYjsCache;

// How often heartbeat checks are performed (server-side connection health monitoring)
// Note: y-websocket client maintains its own keepalive via resyncInterval (20s)
// This server-side heartbeat is for detecting truly dead connections.
//
// Overridable via env (`NOSDESK_WS_HEARTBEAT_MS` /
// `NOSDESK_WS_CLIENT_TIMEOUT_MS`) so integration tests don't sit on
// the 20s/60s wall clock. The Lazy resolves once at first access;
// production code never sets these. Defaults match the original
// constants exactly.
static HEARTBEAT_INTERVAL: once_cell::sync::Lazy<Duration> = once_cell::sync::Lazy::new(|| {
    std::env::var("NOSDESK_WS_HEARTBEAT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(20))
});
static CLIENT_TIMEOUT: once_cell::sync::Lazy<Duration> = once_cell::sync::Lazy::new(|| {
    std::env::var("NOSDESK_WS_CLIENT_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(60))
});
// Minimum time between saves for the same document
const MIN_SAVE_INTERVAL: Duration = Duration::from_secs(5);
// Maximum time a document can have pending changes before forcing a save
const MAX_PENDING_DURATION: Duration = Duration::from_secs(120);
// How long to wait before doing final save on empty room
const EMPTY_ROOM_FINAL_SAVE_DELAY: Duration = Duration::from_secs(2);
// How long an empty (and final-saved) room stays in memory before its
// ownership claim is released and the doc evicted, under multi-instance
// routing. Long enough to absorb a quick close/reopen without churning
// the claim, short enough that an idle doc stops pinning a machine.
// Single-instance mode never evicts (no ownership manager). See
// `docs/realtime-collab-affinity-design.md`.
const EMPTY_ROOM_EVICT_DELAY: Duration = Duration::from_secs(60);
// Document type enum: distinguishes ticket articles, doc pages,
// and collection descriptions. The collection variant binds to
// the new `documentation_collections.description_yjs` column —
// collections own their overview content directly instead of
// pointing at a sentinel "main page".
#[derive(Debug, Clone, Copy, PartialEq)]
enum DocumentType {
    Ticket(i32),
    Documentation(i32),
    Collection(i32),
}

/// Which kind of collaborative resource a doc_id names. The doc_id
/// carries the resource's immutable UUID; the integer id used by the
/// persistence layer is resolved from it (see [`ParsedDocId::resolve`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum DocKind {
    Ticket,
    Documentation,
    Collection,
}

/// Result of parsing a workspace-namespaced doc_id
/// (`ws-{workspace_uuid}_{kind}-{resource_uuid}`). Carries the
/// workspace UUID (so the caller can fail-fast on a cross-workspace
/// request) plus the resource kind + its immutable UUID. The integer
/// [`DocumentType`] the persistence layer needs is produced by
/// [`ParsedDocId::resolve`], which looks the UUID up in the DB — the
/// UUID never recycles, so the resulting doc identity is stable across
/// an integer-id reset.
#[derive(Debug, Clone)]
struct ParsedDocId {
    workspace_uuid: Uuid,
    kind: DocKind,
    resource_uuid: Uuid,
}

impl ParsedDocId {
    /// Resolve the immutable resource UUID to the integer-keyed
    /// [`DocumentType`] used by the persistence + access layers.
    /// Returns `Ok(None)` when no live row has that UUID (resource
    /// deleted, or a stale client holding a recycled-but-different
    /// doc), which the WS handler turns into a clean rejection.
    fn resolve(
        &self,
        conn: &mut crate::db::DbConnection,
    ) -> diesel::QueryResult<Option<DocumentType>> {
        Ok(match self.kind {
            DocKind::Ticket => crate::repository::tickets::id_by_uuid(conn, self.resource_uuid)?
                .map(DocumentType::Ticket),
            DocKind::Documentation => {
                crate::repository::documentation::page_id_by_uuid(conn, self.resource_uuid)?
                    .map(DocumentType::Documentation)
            }
            DocKind::Collection => {
                crate::repository::documentation_collections::collection_id_by_uuid(
                    conn,
                    self.resource_uuid,
                )?
                .map(DocumentType::Collection)
            }
        })
    }
}

/// Errors from parsing the workspace-namespaced doc_id. Each
/// variant maps cleanly to the operator-facing log line + the
/// HTTP status the caller should return.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DocIdParseError {
    /// The doc_id didn't start with the `ws-{uuid}_` namespace
    /// prefix. The legacy bare-id form (`ticket-N`) lands here so
    /// stale clients failing to upgrade are surfaced loudly.
    MissingNamespace,
    /// The namespace was present but its UUID component didn't
    /// parse as a canonical UUID.
    InvalidWorkspaceUuid,
    /// The resource handle after the namespace prefix didn't match
    /// `ticket-N` / `doc-N` / `collection-N`.
    InvalidResource,
}

impl DocumentType {
    /// Parse a workspace-namespaced doc_id of the form
    /// `ws-{workspace_uuid}_{kind}-{id}` (see
    /// `frontend/src/utils/collabDocId.ts` for the matching builder).
    ///
    /// The namespace prefix is mandatory: it's the cache-key
    /// component that orphans stale IndexedDB caches across a
    /// database reset (the workspace UUID changes when the row is
    /// recreated, so the new docId doesn't collide with the old
    /// cache), AND the server-side guard the ws_handler uses to
    /// reject a client requesting another workspace's docId.
    fn from_namespaced_doc_id(doc_id: &str) -> Result<ParsedDocId, DocIdParseError> {
        let after_ws = doc_id
            .strip_prefix("ws-")
            .ok_or(DocIdParseError::MissingNamespace)?;
        // The workspace UUID is canonical hyphenated form; the resource
        // handle follows after a literal `_`. Split on that `_` (UUIDs
        // never contain one) so the resource UUID's own hyphens don't
        // confuse the split.
        let separator = after_ws
            .find('_')
            .ok_or(DocIdParseError::MissingNamespace)?;
        let (uuid_part, rest) = after_ws.split_at(separator);
        let workspace_uuid =
            Uuid::parse_str(uuid_part).map_err(|_| DocIdParseError::InvalidWorkspaceUuid)?;
        // `rest` starts with the `_`; drop it before parsing the
        // `{kind}-{resource_uuid}` handle.
        let resource = &rest[1..];
        let (kind, uuid_str) = if let Some(rest) = resource.strip_prefix("ticket-") {
            (DocKind::Ticket, rest)
        } else if let Some(rest) = resource.strip_prefix("doc-") {
            (DocKind::Documentation, rest)
        } else if let Some(rest) = resource.strip_prefix("collection-") {
            (DocKind::Collection, rest)
        } else {
            return Err(DocIdParseError::InvalidResource);
        };
        let resource_uuid =
            Uuid::parse_str(uuid_str).map_err(|_| DocIdParseError::InvalidResource)?;
        Ok(ParsedDocId {
            workspace_uuid,
            kind,
            resource_uuid,
        })
    }
}

#[cfg(test)]
mod doc_id_tests {
    use super::*;

    const TICKET_UUID: &str = "019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb";
    const WS_UUID: &str = "3f8e9d4c-1234-5678-9abc-def012345678";

    #[test]
    fn parses_ticket_doc_id() {
        let docid = format!("ws-{WS_UUID}_ticket-{TICKET_UUID}");
        let parsed = DocumentType::from_namespaced_doc_id(&docid).expect("should parse");
        assert_eq!(parsed.kind, DocKind::Ticket);
        assert_eq!(parsed.resource_uuid, Uuid::parse_str(TICKET_UUID).unwrap());
        assert_eq!(parsed.workspace_uuid, Uuid::parse_str(WS_UUID).unwrap());
    }

    #[test]
    fn rejects_legacy_bare_id() {
        let err = DocumentType::from_namespaced_doc_id("ticket-42").unwrap_err();
        assert_eq!(err, DocIdParseError::MissingNamespace);
    }

    #[test]
    fn rejects_malformed_workspace_uuid() {
        let err =
            DocumentType::from_namespaced_doc_id(&format!("ws-not-a-uuid_ticket-{TICKET_UUID}"))
                .unwrap_err();
        assert_eq!(err, DocIdParseError::InvalidWorkspaceUuid);
    }

    #[test]
    fn rejects_legacy_integer_resource_id() {
        // A bare integer where the resource UUID is expected (a stale
        // client that didn't upgrade) is rejected, not silently parsed.
        let err =
            DocumentType::from_namespaced_doc_id(&format!("ws-{WS_UUID}_ticket-42")).unwrap_err();
        assert_eq!(err, DocIdParseError::InvalidResource);
    }

    #[test]
    fn rejects_unknown_resource_kind() {
        let err =
            DocumentType::from_namespaced_doc_id(&format!("ws-{WS_UUID}_widget-{TICKET_UUID}"))
                .unwrap_err();
        assert_eq!(err, DocIdParseError::InvalidResource);
    }

    #[test]
    fn parses_each_kind() {
        for (prefix, kind) in [
            ("ticket", DocKind::Ticket),
            ("doc", DocKind::Documentation),
            ("collection", DocKind::Collection),
        ] {
            let parsed = DocumentType::from_namespaced_doc_id(&format!(
                "ws-{WS_UUID}_{prefix}-{TICKET_UUID}"
            ))
            .unwrap();
            assert_eq!(parsed.kind, kind);
            assert_eq!(parsed.resource_uuid, Uuid::parse_str(TICKET_UUID).unwrap());
        }
    }
}

/// Identity resolved enough to authorize collaborative-document
/// access. Tickets gate on visibility (requester/watcher vs staff);
/// documentation pages and collections gate on their ACL with a
/// workspace-admin override. Reuses the same primitives as the REST
/// ticket/documentation read paths and the SSE topic gate so the
/// three can't drift. See security-audit-2026-06.
struct DocAccessor {
    vis: crate::repository::ticket_visibility::VisibilityContext,
    is_workspace_admin: bool,
}

impl DocAccessor {
    /// Build from the `AuthContext` extractor the REST handlers
    /// already destructure (uses the request-resolved workspace role).
    fn from_auth(auth: &AuthContext) -> Self {
        Self {
            vis: crate::repository::ticket_visibility::VisibilityContext::from_auth(auth),
            is_workspace_admin: auth.is_workspace_admin(),
        }
    }

    /// Build from JWT claims when no `AuthContext` is in scope (the
    /// WebSocket handshake validates the token by hand). Resolves the
    /// workspace role from the bootstrap workspace, matching
    /// `VisibilityContext::resolve` and the SSE `SyncViewer`.
    fn from_claims(
        claims: &crate::models::Claims,
        conn: &mut crate::db::DbConnection,
    ) -> Option<Self> {
        let user_uuid = Uuid::parse_str(&claims.sub).ok()?;
        let platform_role = crate::models::PlatformRole::from_db(&claims.platform_role);
        let workspace_role =
            crate::repository::user_helpers::bootstrap_workspace_role(conn, user_uuid);
        let vis = crate::repository::ticket_visibility::VisibilityContext::new(
            user_uuid,
            platform_role,
            workspace_role,
        );
        let is_workspace_admin = platform_role.is_platform_admin()
            || workspace_role.is_some_and(|r| r.meets(crate::models::WorkspaceRole::Admin));
        Some(Self {
            vis,
            is_workspace_admin,
        })
    }
}

/// True when `accessor` may read/edit `document`. The error type is
/// Diesel's so callers can map a DB failure to a 500 and a `false`
/// to a 404 (never 403: a 403 leaks existence, per OWASP IDOR).
fn can_access_document(
    conn: &mut crate::db::DbConnection,
    accessor: &DocAccessor,
    document: &DocumentType,
) -> Result<bool, diesel::result::Error> {
    match document {
        DocumentType::Ticket(id) => {
            crate::repository::ticket_visibility::can_view_ticket(conn, &accessor.vis, *id)
        }
        DocumentType::Documentation(id) => repository::can_user_access_page(
            conn,
            *id,
            &accessor.vis.user_uuid,
            accessor.is_workspace_admin,
        ),
        DocumentType::Collection(id) => {
            repository::documentation_collections::can_user_access_collection(
                conn,
                *id,
                &accessor.vis.user_uuid,
                accessor.is_workspace_admin,
            )
        }
    }
}

/// Gate a ticket-scoped REST handler on visibility: `Ok(())` when the
/// caller may read the ticket, else a ready 404 (404 not 403 so we
/// don't leak existence), or 500 on a check failure.
fn gate_ticket(
    tc: &mut TenantConn,
    auth: &AuthContext,
    ticket_id: i32,
) -> Result<(), HttpResponse> {
    let accessor = DocAccessor::from_auth(auth);
    match tc.run(|conn| can_access_document(conn, &accessor, &DocumentType::Ticket(ticket_id))) {
        Ok(true) => Ok(()),
        Ok(false) => Err(errors::not_found_msg("Ticket not found")),
        Err(e) => {
            error!(ticket_id, error = ?e, "ticket access check failed");
            Err(errors::internal("Failed to check ticket access"))
        }
    }
}

/// Documentation-page equivalent of [`gate_ticket`].
fn gate_doc_page(
    tc: &mut TenantConn,
    auth: &AuthContext,
    page_id: i32,
) -> Result<(), HttpResponse> {
    let accessor = DocAccessor::from_auth(auth);
    match tc.run(|conn| can_access_document(conn, &accessor, &DocumentType::Documentation(page_id)))
    {
        Ok(true) => Ok(()),
        Ok(false) => Err(errors::not_found_msg("Page not found")),
        Err(e) => {
            error!(page_id, error = ?e, "page access check failed");
            Err(errors::internal("Failed to check page access"))
        }
    }
}

// Simple handler to get article content by ticket ID or documentation page ID
pub async fn get_article_content(
    mut tc: TenantConn,
    doc_id: web::Path<String>,
    auth: AuthContext,
) -> impl Responder {
    let doc_id = doc_id.into_inner();
    let clean_doc_id = doc_id.replace("/", "_");

    // Parse the workspace-namespaced doc_id and resolve its immutable
    // resource UUID to the integer-keyed document type. The lookup runs
    // on the RLS-scoped connection, so a UUID belonging to another
    // workspace simply doesn't resolve.
    let parsed = match DocumentType::from_namespaced_doc_id(&clean_doc_id) {
        Ok(p) => p,
        Err(e) => {
            warn!(doc_id = %clean_doc_id, error = ?e, "Invalid document ID format");
            return errors::bad_request(
                "doc_id must be in the workspace-namespaced format ws-{uuid}_{kind}-{uuid}",
            );
        }
    };
    let doc_type = match tc.run(|conn| parsed.resolve(conn)) {
        Ok(Some(dt)) => dt,
        Ok(None) => return errors::not_found_msg("Document not found"),
        Err(e) => {
            error!(doc_id = %clean_doc_id, error = ?e, "Failed to resolve document id");
            return errors::internal("Failed to resolve document");
        }
    };

    // Per-document visibility gate. Without it a restricted member who
    // cannot read ticket N via the REST API could still pull its note
    // body (and revision history) here. Reuses the REST/SSE primitives.
    // See security-audit-2026-06.
    let accessor = DocAccessor::from_auth(&auth);
    match tc.run(|conn| can_access_document(conn, &accessor, &doc_type)) {
        Ok(true) => {}
        Ok(false) => return errors::not_found_msg("Document not found"),
        Err(e) => {
            error!(doc_id = %clean_doc_id, error = ?e, "Document access check failed");
            return errors::internal("Failed to check document access");
        }
    }

    match doc_type {
        DocumentType::Ticket(ticket_id) => {
            // Load Yjs document snapshot from article_contents table (snapshot-based persistence)
            match tc.run(|conn| repository::get_article_content_by_ticket_id(conn, ticket_id)) {
                Ok(article_content) => {
                    debug!(ticket_id, "Retrieved article content");

                    // If yjs_document snapshot exists, encode as base64, otherwise return empty
                    let content_base64 = if let Some(yjs_doc) = article_content.yjs_document {
                        if !yjs_doc.is_empty() {
                            debug!(
                                ticket_id,
                                bytes = yjs_doc.len(),
                                "Loading snapshot from PostgreSQL"
                            );
                            general_purpose::STANDARD.encode(&yjs_doc)
                        } else {
                            debug!(ticket_id, "Empty Yjs document");
                            String::new()
                        }
                    } else {
                        debug!(ticket_id, "No Yjs document snapshot");
                        String::new()
                    };

                    HttpResponse::Ok().json(json!({
                        "content": content_base64,
                        "ticket_id": ticket_id
                    }))
                }
                Err(e) => {
                    debug!(ticket_id, error = ?e, "No article content found");
                    HttpResponse::Ok().json(json!({
                        "content": "",
                        "ticket_id": ticket_id
                    }))
                }
            }
        }
        DocumentType::Documentation(doc_id) => {
            match tc.run(|conn| repository::get_documentation_page(doc_id, conn)) {
                Ok(doc_page) => {
                    debug!(doc_id, "Retrieved documentation page");

                    // If yjs_document exists, encode as base64, otherwise return empty
                    let content_base64 = if let Some(yjs_doc) = doc_page.yjs_document {
                        general_purpose::STANDARD.encode(&yjs_doc)
                    } else {
                        String::new()
                    };

                    HttpResponse::Ok().json(json!({
                        "content": content_base64,
                        "doc_id": doc_id
                    }))
                }
                Err(e) => {
                    debug!(doc_id, error = %e, "No documentation page found");
                    HttpResponse::Ok().json(json!({
                        "content": "",
                        "doc_id": doc_id
                    }))
                }
            }
        }
        DocumentType::Collection(collection_id) => {
            match tc.run(|conn| {
                repository::documentation_collections::get_collection(conn, collection_id)
            }) {
                Ok(c) => {
                    let content_base64 = c
                        .description_yjs
                        .as_ref()
                        .map(|d| general_purpose::STANDARD.encode(d))
                        .unwrap_or_default();
                    HttpResponse::Ok().json(json!({
                        "content": content_base64,
                        "collection_id": collection_id
                    }))
                }
                Err(e) => {
                    debug!(collection_id, error = %e, "No collection found");
                    HttpResponse::Ok().json(json!({
                        "content": "",
                        "collection_id": collection_id
                    }))
                }
            }
        }
    }
}

// ============= WebSocket implementation =============

// Document state tracking
#[derive(Clone)]
struct DocumentState {
    awareness: Arc<Awareness>,
    last_saved: Instant,
    has_pending_changes: bool,
    pending_since: Option<Instant>,
    sync_message_count: u32,
    room_empty_since: Option<Instant>, // Track when room became empty
    final_save_completed: bool,        // Track if final save was done
    // Snapshot tracking (for version history)
    update_counter: u32,   // Total updates since document creation
    last_snapshot_at: u32, // Update count when last snapshot created
    contributors: std::collections::HashSet<Uuid>, // Contributors since last snapshot (only added on actual content changes)
    /// `update_counter` when the last crash-recovery checkpoint was
    /// written to `yjs_snapshots`. The checkpoint loop only writes when
    /// `update_counter > last_checkpoint_at`, so an idle document never
    /// re-checkpoints. Distinct from `last_snapshot_at` (version-history
    /// revisions), which is session-based.
    last_checkpoint_at: u32,
    /// Workspace that owns this document. Set at session open
    /// from the requesting user's `WorkspaceContext` (subdomain
    /// routing). The background save / snapshot loop reads this
    /// to pin the per-doc actor so RLS-enforced writes hit the
    /// correct workspace's rows.
    workspace_id: i32,
    /// Fencing token from this machine's ownership claim on the doc
    /// (Phase 2 affinity). Stamped on every durable snapshot write so a
    /// stale owner (whose lease expired under a GC pause) is rejected.
    /// `None` in single-instance mode and in the Redis-down degraded
    /// case, where writes are unconditional (today's behaviour). See
    /// `docs/realtime-collab-affinity-design.md`.
    fence: Option<i64>,
    /// Integer-keyed document type, resolved once from the doc_id's
    /// immutable resource UUID at open. The save / snapshot loops read
    /// this instead of re-parsing the doc_id, so they never need a DB
    /// round-trip to learn which table to persist to.
    doc_type: DocumentType,
}

impl DocumentState {
    fn new(
        awareness: Arc<Awareness>,
        workspace_id: i32,
        fence: Option<i64>,
        doc_type: DocumentType,
    ) -> Self {
        Self {
            awareness,
            last_saved: Instant::now(),
            has_pending_changes: false,
            pending_since: None,
            sync_message_count: 0,
            room_empty_since: None,
            final_save_completed: false,
            // Initialize snapshot tracking
            update_counter: 0,
            last_snapshot_at: 0,
            contributors: std::collections::HashSet::new(),
            last_checkpoint_at: 0,
            workspace_id,
            fence,
            doc_type,
        }
    }

    fn mark_changed(&mut self) {
        if !self.has_pending_changes {
            self.has_pending_changes = true;
            self.pending_since = Some(Instant::now());
        }
        self.sync_message_count += 1;
        self.update_counter += 1; // Track total updates for snapshot scheduling
                                  // Note: has_changes_since_last_revision is set separately only when content actually changes

        // Reset room empty tracking since there's activity
        self.room_empty_since = None;
        self.final_save_completed = false;
    }

    fn mark_saved(&mut self) {
        self.last_saved = Instant::now();
        self.has_pending_changes = false;
        self.pending_since = None;
        self.sync_message_count = 0;
    }

    fn mark_room_empty(&mut self) {
        if self.room_empty_since.is_none() {
            self.room_empty_since = Some(Instant::now());
            self.final_save_completed = false;
        }
    }

    fn mark_room_active(&mut self) {
        self.room_empty_since = None;
        self.final_save_completed = false;
    }

    fn mark_final_save_completed(&mut self) {
        self.final_save_completed = true;
    }

    fn should_save(&self) -> bool {
        if !self.has_pending_changes {
            return false;
        }

        let now = Instant::now();

        // Save if enough time has passed since last save
        if now.duration_since(self.last_saved) >= MIN_SAVE_INTERVAL {
            return true;
        }

        // Force save if changes have been pending too long
        if let Some(pending_since) = self.pending_since {
            if now.duration_since(pending_since) >= MAX_PENDING_DURATION {
                return true;
            }
        }

        // Force save after 10 sync messages to prevent data loss
        if self.sync_message_count >= 10 {
            return true;
        }

        false
    }

    fn should_do_final_save(&self) -> bool {
        // Only do final save if room has been empty for a bit, changes exist, and final save not yet done
        if let Some(empty_since) = self.room_empty_since {
            let now = Instant::now();
            return !self.final_save_completed
                && (self.has_pending_changes
                    || now.duration_since(empty_since) < Duration::from_secs(5))
                && now.duration_since(empty_since) >= EMPTY_ROOM_FINAL_SAVE_DELAY;
        }
        false
    }

    // Snapshot management methods
    fn should_create_snapshot(&self) -> bool {
        // Session-based revisions: snapshots are only created when editing sessions end
        // (when room becomes empty), not based on update count thresholds.
        // This provides more meaningful revision history based on actual editing sessions.
        false
    }

    fn add_contributor(&mut self, user_uuid: Uuid) {
        self.contributors.insert(user_uuid);
    }

    fn reset_snapshot_tracking(&mut self) {
        self.last_snapshot_at = self.update_counter;
        self.contributors.clear();
    }
}

// Create app state to manage active documents and awareness
type DocumentId = String;
type SessionId = String;

/// Per-session bookkeeping kept on the collaboration side. The
/// user identity + ticket presence live in
/// `services::presence::PresenceRegistry`; this map only carries
/// what the transport needs (the outbound channel and the last
/// activity Instant for stale-cleanup).
///
/// `tx` is the write side of the per-connection mpsc the
/// session_task drains and forwards to the wire. Cloning the
/// Sender is cheap; `broadcast` collects clones under the read
/// lock and pushes after release.
struct SessionInfo {
    tx: mpsc::UnboundedSender<Bytes>,
    last_active: Instant,
    /// User who owns this session. Forwarded to the presence
    /// registry on add / remove so the registry can deduplicate
    /// multi-tab.
    user_uuid: Uuid,
    /// Yjs `clientID` (sniffed from the first inbound awareness
    /// frame; see `process_inbound_binary`). `None` until the
    /// client has sent at least one awareness update — which is
    /// the case for the initial handshake before the cursor lands.
    ///
    /// `cleanup_stale_sessions` reads this so it can call
    /// `Awareness::remove_state` and broadcast the tombstone
    /// update before dropping the session row. Without it, a
    /// stale session would vanish from the in-memory map but
    /// its Yjs awareness state would persist until the next
    /// peer-side timeout (~30s on the JS client, indefinite on
    /// yrs peers), surfacing as ghost cursors for other viewers.
    yjs_client_id: Option<u64>,
    /// Eviction signal for this session. The session_task selects on
    /// `cancel.notified()`; when the owning machine evicts the document
    /// (it lost the ownership lease, Phase 2 affinity), it notifies every
    /// session in the room so they tear down and the client reconnects,
    /// re-routing to the new owner. Unused in single-instance mode.
    cancel: Arc<Notify>,
    /// Integer-keyed document type resolved from the doc_id's immutable
    /// resource UUID at session open. The presence sites
    /// (`update_session_activity`, `remove_session`,
    /// `cleanup_stale_sessions`) read this instead of re-parsing the
    /// doc_id, so they need no DB round-trip and presence stays keyed on
    /// the stable ticket id.
    doc_type: DocumentType,
}

type RoomSessions = HashMap<DocumentId, HashMap<SessionId, SessionInfo>>;
type RoomSessionStore = Arc<RwLock<RoomSessions>>;
type DocumentStore = Arc<RwLock<HashMap<DocumentId, DocumentState>>>;

// Define shared app state for WebSocket connections
#[derive(Clone)]
pub struct YjsAppState {
    documents: DocumentStore,
    sessions: RoomSessionStore,
    pool: web::Data<crate::db::Pool>,
    redis_cache: Arc<RedisYjsCache>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    /// Search service handle so the periodic + on-disconnect Yjs
    /// saves can fire the indexing observers and keep the search
    /// index in sync with body edits typed into the collaborative
    /// editor. Without this every Yjs save would leave the index
    /// stale until a metadata change triggered a reindex.
    search_service: Arc<crate::services::search::SearchService>,
    /// Per-(user, ticket) presence state. Single source of truth
    /// for the avatar stack on the ticket detail page; gates the
    /// per-user "appear away" preference via its visibility
    /// resolver.
    presence: Arc<crate::services::presence::PresenceRegistry>,
    /// Per-document ownership claims for multi-instance routing
    /// (Phase 2 affinity). `None` in single-instance mode
    /// (`NOSDESK_COLLAB_ROUTING` unset / `single`), in which case
    /// the routing layer is inert and every doc is served locally.
    /// See `docs/realtime-collab-affinity-design.md`.
    ownership: Option<Arc<crate::services::collab_ownership::CollabOwnership>>,
    /// Which routing mode this machine runs in. `Single` when
    /// `ownership` is `None`; `FlyReplay` / `DirectAddress` when set.
    routing_mode: CollabRoutingMode,
}

/// Routing mode lives with the ownership manager it configures; re-export
/// it here so `route()` / `YjsAppState` and call sites that reference
/// `handlers::collaboration::CollabRoutingMode` keep resolving.
pub use crate::services::collab_ownership::CollabRoutingMode;

/// Routing decision for an incoming WebSocket before the upgrade.
pub enum CollabRoute {
    /// This machine owns the document (or routing is single-instance):
    /// proceed with the upgrade here. Carries the ownership claim's
    /// fencing token (`None` in single-instance / Redis-down mode) to
    /// stamp on this document's snapshot writes.
    Local(Option<i64>),
    /// Another machine owns the document: the caller must steer the
    /// connection there (on fly, a `fly-replay: instance=<id>` header)
    /// without negotiating the upgrade.
    ReplayTo(String),
    /// Direct-address mode: this is not the owner, so the client must
    /// re-run the handshake to learn the owner's address. (The handshake
    /// endpoint, not the WS handler, resolves the address.)
    Rehandshake,
}

impl YjsAppState {
    pub fn new(
        pool: web::Data<crate::db::Pool>,
        redis_cache: Arc<RedisYjsCache>,
        sse_state: web::Data<crate::handlers::sse::SseState>,
        search_service: Arc<crate::services::search::SearchService>,
        ownership: Option<Arc<crate::services::collab_ownership::CollabOwnership>>,
        routing_mode: CollabRoutingMode,
    ) -> Self {
        let state = YjsAppState {
            documents: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pool,
            redis_cache,
            sse_state,
            search_service,
            presence: Arc::new(
                crate::services::presence::PresenceRegistry::with_default_resolver(),
            ),
            ownership,
            routing_mode,
        };
        // Publish this machine's address to the registry immediately
        // (direct-address mode), so a doc owned here is reachable before
        // the first maintenance tick. No-op in other modes.
        if let Some(ownership) = &state.ownership {
            let ownership = ownership.clone();
            actix_web::rt::spawn(async move { ownership.register_self().await });
        }
        // Start the periodic cleanup and save task. `actix_web::rt::spawn`
        // schedules onto the actix runtime, same as the old
        // `actix::spawn` did, but without the actix-actor framework
        // dependency.
        let state_clone = state.clone();
        actix_web::rt::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                state_clone.cleanup_stale_sessions().await;
                state_clone.save_all_active_documents().await;
                state_clone.renew_owned_documents().await;
                // Refresh the machine-address registry TTL (direct-address
                // mode); no-op otherwise.
                if let Some(ownership) = &state_clone.ownership {
                    ownership.register_self().await;
                }
            }
        });

        // Crash-recovery checkpoint loop (Phase 2). Cheap binary
        // checkpoints to `yjs_snapshots` on a faster cadence than the 30s
        // save above, so a hard crash (SIGKILL / OOM) loses seconds rather
        // than the whole save interval. Graceful shutdown is covered
        // separately by `flush_all_dirty`.
        let checkpoint_state = state.clone();
        actix_web::rt::spawn(async move {
            let secs = std::env::var("NOSDESK_COLLAB_CHECKPOINT_SECS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|s| *s > 0)
                .unwrap_or(10);
            let mut interval = tokio::time::interval(Duration::from_secs(secs));
            loop {
                interval.tick().await;
                checkpoint_state.checkpoint_all_active_documents().await;
            }
        });
        state
    }

    /// Decide how to handle an incoming WebSocket for `doc_id` before
    /// upgrading. In single-instance mode (no ownership manager) this is
    /// always `Local`. Otherwise it resolves (and, if free, claims) the
    /// owner: `Local` if this machine owns it, else `ReplayTo(owner)` so
    /// the caller can route the connection to the owning machine.
    pub async fn route(&self, doc_id: &str) -> CollabRoute {
        let Some(ownership) = &self.ownership else {
            return CollabRoute::Local(None);
        };
        let resolution = ownership.resolve_or_claim(doc_id).await;
        if resolution.is_local {
            return CollabRoute::Local(resolution.fence);
        }
        match self.routing_mode {
            // Unreachable: Single mode has no ownership manager.
            CollabRoutingMode::Single => CollabRoute::Local(None),
            CollabRoutingMode::FlyReplay => CollabRoute::ReplayTo(resolution.owner),
            // The handshake endpoint hands the client the owner's
            // address; a WS that still lands here is stale.
            CollabRoutingMode::DirectAddress => CollabRoute::Rehandshake,
        }
    }

    /// Resolve the WebSocket URL a client should connect to for
    /// `doc_id`, used by the handshake endpoint. In single / fly-replay
    /// mode this is the relative path on the current host (fly-replay
    /// then routes to the owner). In direct-address mode it is the
    /// absolute URL of the owning machine (claiming it for the contacted
    /// machine if currently unowned), so the client connects straight to
    /// the owner and bypasses any load-balancer reshuffling. Returns
    /// `None` only when the owner's address is unknown (owner dead /
    /// unregistered), signalling the client to retry.
    async fn resolve_ws_url(&self, doc_id: &str) -> Option<String> {
        let relative = format!("/api/collaboration/ws/{doc_id}");
        let Some(ownership) = &self.ownership else {
            return Some(relative);
        };
        match self.routing_mode {
            CollabRoutingMode::Single | CollabRoutingMode::FlyReplay => Some(relative),
            CollabRoutingMode::DirectAddress => {
                let resolution = ownership.resolve_or_claim(doc_id).await;
                let base = if resolution.is_local {
                    ownership.address().map(|s| s.to_string())
                } else {
                    ownership.owner_address(&resolution.owner).await
                };
                base.map(|b| format!("{}/api/collaboration/ws/{doc_id}", b.trim_end_matches('/')))
            }
        }
    }

    /// Renew the ownership claim for every document held in memory on
    /// this machine, and evict any whose claim was lost.
    ///
    /// A document is in `self.documents` only because this machine
    /// loaded it to serve a connection, so the in-memory set is exactly
    /// the set this machine owns. If a renewal reports the claim was
    /// lost (another machine took over after our lease expired under a
    /// GC pause), this machine must immediately stop being an authority
    /// for that doc: it evicts the doc and tears down its sessions so
    /// clients reconnect and re-route to the new owner. Fencing protects
    /// the durable snapshot during the brief overlap. No-op in
    /// single-instance mode.
    async fn renew_owned_documents(&self) {
        let Some(ownership) = &self.ownership else {
            return;
        };
        let doc_ids: Vec<String> = {
            let documents = self.documents.read().await;
            documents.keys().cloned().collect()
        };
        for doc_id in doc_ids {
            if !ownership.renew(&doc_id).await {
                warn!(doc_id = %doc_id, "Lost ownership claim; evicting document and its sessions");
                // Claim already lost, so don't release (compare-and-del
                // would no-op anyway); just tear down locally.
                self.evict_document(&doc_id).await;
            }
        }
    }

    /// Drop a document from this machine: remove it from the in-memory
    /// store and tear down every session in its room (signalling each
    /// session_task to stop via its cancel `Notify`). Used by both
    /// lost-lease eviction and idle release. After this the room is gone
    /// from both maps; clients whose sockets close will reconnect and
    /// re-route to the current owner.
    async fn evict_document(&self, doc_id: &str) {
        {
            let mut documents = self.documents.write().await;
            documents.remove(doc_id);
        }
        let room = {
            let mut sessions = self.sessions.write().await;
            sessions.remove(doc_id)
        };
        if let Some(room) = room {
            for (_session_id, info) in room {
                info.cancel.notify_one();
            }
        }
    }

    // Save all active documents
    async fn save_all_active_documents(&self) {
        let mut documents = self.documents.write().await;
        let mut saved_count = 0;
        let mut final_saved_count = 0;
        let mut snapshot_count = 0;
        // Idle docs to release + evict after the save pass (multi-instance
        // only). Collected here, acted on after the documents lock drops.
        let mut to_evict: Vec<String> = Vec::new();

        for (doc_id, doc_state) in documents.iter_mut() {
            let workspace_id = doc_state.workspace_id;
            // Regular saves for active documents
            if doc_state.should_save() {
                debug!(doc_id = %doc_id, "Saving document with pending changes");
                // Mid-session save: not the end of editing, so no
                // content-saved emit yet.
                self.save_document_internal(
                    doc_id,
                    &doc_state.awareness,
                    workspace_id,
                    doc_state.fence,
                    doc_state.doc_type,
                    false,
                );
                doc_state.mark_saved();
                saved_count += 1;
            }

            // Check for snapshot creation (every 500 updates)
            if doc_state.should_create_snapshot() {
                debug!(doc_id = %doc_id, updates_since_snapshot = doc_state.update_counter - doc_state.last_snapshot_at,
                    "Snapshot threshold reached");

                // Clone contributors before passing to async function
                let contributors = doc_state.contributors.clone();
                self.create_snapshot_revision(
                    doc_id,
                    &doc_state.awareness,
                    contributors,
                    workspace_id,
                    doc_state.doc_type,
                );
                doc_state.reset_snapshot_tracking();
                snapshot_count += 1;
            }

            // Final save for empty rooms
            if doc_state.should_do_final_save() {
                debug!(doc_id = %doc_id, "Performing final save for empty room");
                // End-of-session save: emit content-saved when the session
                // actually changed content (contributors are added only on
                // real change), so the search replicator re-indexes.
                self.save_document_internal(
                    doc_id,
                    &doc_state.awareness,
                    workspace_id,
                    doc_state.fence,
                    doc_state.doc_type,
                    !doc_state.contributors.is_empty(),
                );
                doc_state.mark_saved();
                doc_state.mark_final_save_completed();
                final_saved_count += 1;

                // Create revision at end of editing session if there were content changes
                if !doc_state.contributors.is_empty() {
                    debug!(doc_id = %doc_id, "Creating session-end revision");
                    let contributors = doc_state.contributors.clone();
                    self.create_snapshot_revision(
                        doc_id,
                        &doc_state.awareness,
                        contributors,
                        workspace_id,
                        doc_state.doc_type,
                    );
                    doc_state.reset_snapshot_tracking();
                    snapshot_count += 1;
                }
            }

            // Single-instance: keep documents in memory indefinitely.
            // They hold the authoritative live state; the DB is only cold
            // storage (restart recovery). Keeping them avoids a race where
            // a user reconnects before an async save completes.
            // See: https://discuss.yjs.dev/t/correct-way-to-implement-version-history-like-google-doc/1691
            //
            // Multi-instance: an empty, final-saved room that has been idle
            // past EMPTY_ROOM_EVICT_DELAY is released so another machine can
            // own it. We only collect candidates here; the actual release +
            // eviction happens after the documents lock drops, and re-checks
            // the room is still empty (a session may have rejoined).
            if self.ownership.is_some() && doc_state.final_save_completed {
                if let Some(empty_since) = doc_state.room_empty_since {
                    if empty_since.elapsed() >= EMPTY_ROOM_EVICT_DELAY {
                        to_evict.push(doc_id.clone());
                    }
                }
            }
        }

        if saved_count > 0 || final_saved_count > 0 || snapshot_count > 0 {
            info!(
                saves = saved_count,
                final_saves = final_saved_count,
                snapshots = snapshot_count,
                "Periodic maintenance completed"
            );
        }

        // Release + evict idle docs (multi-instance). Drop the documents
        // lock first: evict_document re-acquires it.
        drop(documents);
        for doc_id in to_evict {
            let still_empty = {
                let sessions = self.sessions.read().await;
                sessions.get(&doc_id).map(|r| r.is_empty()).unwrap_or(true)
            };
            if !still_empty {
                continue;
            }
            if let Some(ownership) = &self.ownership {
                ownership.release(&doc_id).await;
            }
            debug!(doc_id = %doc_id, "Released idle document ownership claim and evicted");
            self.evict_document(&doc_id).await;
        }
    }

    // Get or create awareness for a document
    /// Get the in-memory awareness for `doc_id`, loading + creating it if
    /// absent. `fence` is the ownership claim's token from the routing
    /// step; it is applied only when this call creates the document
    /// (recorded on `DocumentState` for snapshot-write fencing). Callers
    /// that reach an already-loaded doc, or that aren't a fresh claim,
    /// pass `None`.
    async fn get_or_create_awareness(
        &self,
        doc_id: &str,
        workspace_id: i32,
        fence: Option<i64>,
        doc_type: DocumentType,
    ) -> Arc<Awareness> {
        let mut documents = self.documents.write().await;

        if let Some(doc_state) = documents.get_mut(doc_id) {
            // Document exists in memory - reuse it (this is the live state!)
            // Reset the empty room timer since there's activity
            doc_state.mark_room_active();
            Arc::clone(&doc_state.awareness)
        } else {
            debug!(doc_id = %doc_id, "Document not in memory - checking Redis cache");

            // Build the document with the shared server conventions (GC
            // off, a deterministic 53-bit client id stable across
            // restarts, and the "prosemirror" root declared up front).
            // apply_update() below merges the loaded state into it.
            let doc = new_server_doc(doc_id);
            let mut awareness = Awareness::new(doc);

            let mut loaded_from_redis = false;
            let mut loaded_from_postgres = false;

            // STEP 1: Try to load from Redis (hot cache - survives restarts)
            if let Some(redis_data) = self.redis_cache.get_document(doc_id).await {
                debug!(doc_id = %doc_id, bytes = redis_data.len(), "Attempting to load document from Redis");

                if let Ok(update) = Update::decode_v1(&redis_data) {
                    let apply_result = {
                        let mut txn = awareness.doc_mut().transact_mut();
                        txn.apply_update(update)
                    };

                    if let Err(e) = apply_result {
                        error!(doc_id = %doc_id, error = ?e, "Error applying Redis state");
                        // Delete corrupted entry from Redis
                        warn!(doc_id = %doc_id, "Deleting corrupted Redis entry");
                        self.redis_cache.delete_document(doc_id).await;
                    } else {
                        debug!(doc_id = %doc_id, "Successfully loaded document from Redis cache");
                        loaded_from_redis = true;

                        // Diagnostic: Verify content
                        let preview = get_content_preview(&awareness, 50);
                        trace!(doc_id = %doc_id, preview = %preview, "Redis content loaded");
                        log_document_root_types(&awareness, doc_id);
                    }
                } else {
                    warn!(doc_id = %doc_id, "Failed to decode Redis data - deleting corrupted entry");
                    // Delete corrupted entry from Redis so it doesn't block future loads
                    self.redis_cache.delete_document(doc_id).await;
                }
            }

            // STEP 2: Fall back to PostgreSQL (cold storage) if Redis didn't have it
            if !loaded_from_redis {
                debug!(doc_id = %doc_id, "Redis cache miss - checking PostgreSQL");

                // doc_type was resolved from the doc_id's resource UUID
                // at the connection boundary and threaded in here.
                {
                    match self.pool.get() {
                        Ok(mut conn) => {
                            // Per-doc reads run RLS-enforced under the
                            // workspace-pinned session actor resolved
                            // at WebSocket open. If the user's
                            // WorkspaceContext doesn't grant access to
                            // the doc, RLS returns NotFound and we
                            // fall through to the "new document" path.
                            let session_actor = yjs_session_actor(workspace_id);
                            match doc_type {
                                DocumentType::Ticket(ticket_id) => {
                                    // Load Yjs document snapshot from article_contents table (snapshot-based persistence)
                                    match session::with_actor_context(
                                        &mut conn,
                                        &session_actor,
                                        |conn| {
                                            repository::get_article_content_by_ticket_id(
                                                conn, ticket_id,
                                            )
                                        },
                                    ) {
                                        Ok(article_content) => {
                                            if let Some(yjs_doc) = article_content.yjs_document {
                                                if !yjs_doc.is_empty() {
                                                    debug!(
                                                        ticket_id,
                                                        bytes = yjs_doc.len(),
                                                        "Loading snapshot from PostgreSQL"
                                                    );

                                                    if let Ok(update) = Update::decode_v1(&yjs_doc)
                                                    {
                                                        let apply_result = {
                                                            let mut txn =
                                                                awareness.doc_mut().transact_mut();
                                                            txn.apply_update(update)
                                                        };

                                                        if let Err(e) = apply_result {
                                                            error!(ticket_id, error = ?e, "Error applying PostgreSQL snapshot");
                                                        } else {
                                                            debug!(ticket_id, "Successfully loaded snapshot from PostgreSQL");
                                                            loaded_from_postgres = true;

                                                            // Cache in Redis for future fast access
                                                            self.redis_cache
                                                                .set_document(doc_id, &yjs_doc)
                                                                .await;

                                                            // Diagnostic: Check content
                                                            let preview = get_content_preview(
                                                                &awareness, 100,
                                                            );
                                                            trace!(ticket_id, preview = %preview, "PostgreSQL content loaded");
                                                            log_document_root_types(
                                                                &awareness, doc_id,
                                                            );
                                                        }
                                                    } else {
                                                        error!(
                                                            ticket_id,
                                                            "Failed to decode PostgreSQL snapshot"
                                                        );
                                                    }
                                                } else {
                                                    debug!(ticket_id, "Empty Yjs document");
                                                }
                                            } else {
                                                debug!(ticket_id, "No Yjs document snapshot");
                                            }
                                        }
                                        Err(e) => {
                                            debug!(ticket_id, error = ?e, "No article content found");
                                        }
                                    }
                                }
                                DocumentType::Documentation(doc_page_id) => {
                                    // Load Yjs document snapshot from documentation_pages table (snapshot-based persistence)
                                    match session::with_actor_context(
                                        &mut conn,
                                        &session_actor,
                                        |conn| {
                                            repository::get_documentation_page(doc_page_id, conn)
                                        },
                                    ) {
                                        Ok(doc_page) => {
                                            if let Some(yjs_doc) = doc_page.yjs_document {
                                                if !yjs_doc.is_empty() {
                                                    debug!(
                                                        doc_page_id,
                                                        bytes = yjs_doc.len(),
                                                        "Loading from PostgreSQL"
                                                    );

                                                    if let Ok(update) = Update::decode_v1(&yjs_doc)
                                                    {
                                                        let apply_result = {
                                                            let mut txn =
                                                                awareness.doc_mut().transact_mut();
                                                            txn.apply_update(update)
                                                        };

                                                        if let Err(e) = apply_result {
                                                            error!(doc_page_id, error = ?e, "Error applying PostgreSQL state");
                                                        } else {
                                                            debug!(doc_page_id, "Successfully loaded documentation from PostgreSQL");
                                                            loaded_from_postgres = true;

                                                            // Cache in Redis
                                                            self.redis_cache
                                                                .set_document(doc_id, &yjs_doc)
                                                                .await;

                                                            // Diagnostic: Check what's actually in the document
                                                            let preview = get_content_preview(
                                                                &awareness, 100,
                                                            );
                                                            trace!(doc_page_id, preview = %preview, "PostgreSQL content loaded");
                                                        }
                                                    } else {
                                                        error!(doc_page_id, "Failed to decode Yjs update from PostgreSQL");
                                                    }
                                                } else {
                                                    debug!(doc_page_id, "New documentation page - no existing Yjs content");
                                                }
                                            } else {
                                                debug!(doc_page_id, "New documentation page - no existing Yjs content");
                                            }
                                        }
                                        Err(e) => {
                                            debug!(doc_page_id, error = ?e, "No existing documentation page in PostgreSQL");
                                        }
                                    }
                                }
                                DocumentType::Collection(collection_id) => {
                                    // Load Yjs snapshot from documentation_collections.description_yjs.
                                    match session::with_actor_context(
                                        &mut conn,
                                        &session_actor,
                                        |conn| {
                                            repository::documentation_collections::get_collection(
                                                conn,
                                                collection_id,
                                            )
                                        },
                                    ) {
                                        Ok(c) => {
                                            if let Some(yjs_doc) = c.description_yjs {
                                                if !yjs_doc.is_empty() {
                                                    debug!(collection_id, bytes = yjs_doc.len(), "Loading collection description from PostgreSQL");
                                                    if let Ok(update) = Update::decode_v1(&yjs_doc)
                                                    {
                                                        let apply_result = {
                                                            let mut txn =
                                                                awareness.doc_mut().transact_mut();
                                                            txn.apply_update(update)
                                                        };
                                                        if let Err(e) = apply_result {
                                                            error!(collection_id, error = ?e, "Error applying collection description state");
                                                        } else {
                                                            loaded_from_postgres = true;
                                                            self.redis_cache
                                                                .set_document(doc_id, &yjs_doc)
                                                                .await;
                                                        }
                                                    } else {
                                                        error!(collection_id, "Failed to decode Yjs update for collection");
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            debug!(collection_id, error = ?e, "No collection in PostgreSQL");
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!(doc_id = %doc_id, error = ?e, "Database connection error");
                        }
                    }
                }
            }

            // STEP 3: Merge the latest crash-recovery checkpoint (Phase 2).
            // Cheap binary checkpoints land in `yjs_snapshots` between the
            // heavier article_contents saves, so a hard crash loses
            // seconds. Yjs merges are conflict-free + idempotent, so
            // applying the checkpoint on top of whatever loaded above can
            // only add missing ops, never regress (no newest-wins
            // comparison needed). Skipped on a Redis hit: Redis is written
            // on every save, so it is already at least as fresh as any
            // checkpoint.
            let mut loaded_from_checkpoint = false;
            if !loaded_from_redis {
                if let Ok(mut conn) = self.pool.get() {
                    let session_actor = yjs_session_actor(workspace_id);
                    let latest = session::with_actor_context(&mut conn, &session_actor, |conn| {
                        repository::yjs_snapshots::latest_for_document(conn, workspace_id, doc_id)
                    });
                    if let Ok(Some(bytes)) = latest {
                        match Update::decode_v1(&bytes) {
                            Ok(update) => {
                                let apply_result = {
                                    let mut txn = awareness.doc_mut().transact_mut();
                                    txn.apply_update(update)
                                };
                                match apply_result {
                                    Ok(()) => {
                                        loaded_from_checkpoint = true;
                                        debug!(doc_id = %doc_id, bytes = bytes.len(), "Merged crash-recovery checkpoint");
                                    }
                                    Err(e) => {
                                        error!(doc_id = %doc_id, error = ?e, "Error applying crash-recovery checkpoint")
                                    }
                                }
                            }
                            Err(e) => {
                                error!(doc_id = %doc_id, error = ?e, "Failed to decode crash-recovery checkpoint")
                            }
                        }
                    }
                }
            }

            // For NEW documents only (no existing data), initialize the prosemirror XmlFragment
            // This ensures new documents have the proper root type structure for ProseMirror
            if !loaded_from_redis && !loaded_from_postgres && !loaded_from_checkpoint {
                let mut txn = awareness.doc_mut().transact_mut();
                let _ = txn.get_or_insert_xml_fragment("prosemirror");
                debug!(doc_id = %doc_id, "Initialized 'prosemirror' XmlFragment for NEW document");
            }

            // Log final state after loading attempts
            let preview = get_content_preview(&awareness, 100);
            if loaded_from_redis || loaded_from_postgres || loaded_from_checkpoint {
                debug!(doc_id = %doc_id, preview = %preview, "Document loaded");
                log_document_root_types(&awareness, doc_id);
            } else {
                debug!(doc_id = %doc_id, preview = %preview, "New document created");
            }

            let awareness_arc = Arc::new(awareness);
            let doc_state =
                DocumentState::new(Arc::clone(&awareness_arc), workspace_id, fence, doc_type);
            documents.insert(doc_id.to_string(), doc_state);
            awareness_arc
        }
    }

    /// Non-creating lookup of an in-memory document's awareness. Used by
    /// paths that must never bring a document back into memory: inbound
    /// frame handling and disconnect cleanup. The document is always
    /// created at session start (`get_or_create_awareness` in the
    /// initial sync), so by the time these run it exists, unless it was
    /// evicted (ownership handoff) in the meantime, in which case the
    /// caller drops the work rather than resurrecting an unowned doc that
    /// would then write to storage unfenced.
    async fn get_awareness(&self, doc_id: &str) -> Option<Arc<Awareness>> {
        let documents = self.documents.read().await;
        documents.get(doc_id).map(|s| Arc::clone(&s.awareness))
    }

    // Mark document as having pending changes
    async fn mark_document_changed(&self, doc_id: &str) {
        let mut documents = self.documents.write().await;
        if let Some(doc_state) = documents.get_mut(doc_id) {
            doc_state.mark_changed();
        }
    }

    /// Replace the document with a new one (used for restoring revisions)
    /// This creates a new Awareness with the new Doc and replaces the existing one
    async fn replace_document(
        &self,
        doc_id: &str,
        new_doc: Doc,
        workspace_id: i32,
        doc_type: DocumentType,
    ) {
        let mut documents = self.documents.write().await;

        // Create new Awareness with the new Doc
        let awareness = Awareness::new(new_doc);

        // Initialize awareness with basic server info
        let local_state = r#"{"server": true, "name": "Server"}"#;
        let _ = awareness.set_local_state(local_state);

        let awareness = Arc::new(awareness);

        if let Some(doc_state) = documents.get_mut(doc_id) {
            // Replace the awareness with the new one
            doc_state.awareness = Arc::clone(&awareness);
            doc_state.mark_changed();
            info!(doc_id = %doc_id, "Replaced document with restored revision");
        } else {
            // Document doesn't exist in memory, create it. Restore runs
            // on the owning machine but doesn't carry the claim fence
            // here, so the snapshot writes unconditionally (None); the
            // explicit admin restore is not the stale-owner case fencing
            // guards against.
            let doc_state =
                DocumentState::new(Arc::clone(&awareness), workspace_id, None, doc_type);
            documents.insert(doc_id.to_string(), doc_state);
            info!(doc_id = %doc_id, "Created new document from restored revision");
        }
    }

    // Track contributor for version history
    async fn add_contributor(&self, doc_id: &str, user_uuid: Uuid) {
        let mut documents = self.documents.write().await;
        if let Some(doc_state) = documents.get_mut(doc_id) {
            doc_state.add_contributor(user_uuid);
        }
    }

    /// Emit a `ViewersChanged` SSE event for one ticket with the
    /// current viewer set from the presence registry. Visibility
    /// resolution is applied inside the registry, so any "appear
    /// away" hidden users are already filtered.
    async fn emit_viewers_changed(&self, ticket_id: i32) {
        let viewers = self.presence.viewers_on_ticket(ticket_id);
        self.sse_state
            .broadcast_event(crate::handlers::sse::SseEvent::ViewersChanged {
                ticket_id,
                viewers,
                timestamp: chrono::Utc::now(),
            })
            .await;
    }

    // Register session
    async fn register_session(
        &self,
        doc_id: &str,
        session_id: &str,
        tx: mpsc::UnboundedSender<Bytes>,
        user_uuid: Uuid,
        cancel: Arc<Notify>,
        doc_type: DocumentType,
    ) {
        let mut sessions = self.sessions.write().await;

        // Get or create the room for this document
        let room = sessions
            .entry(doc_id.to_string())
            .or_insert_with(HashMap::new);

        // Add this session to the room with current timestamp. The
        // Yjs `clientID` is initially `None` and gets filled in by
        // `set_session_yjs_client_id` on the first inbound
        // awareness frame from this session — see the sniff at the
        // top of `process_inbound_binary`.
        room.insert(
            session_id.to_string(),
            SessionInfo {
                tx,
                last_active: Instant::now(),
                user_uuid,
                yjs_client_id: None,
                cancel,
                doc_type,
            },
        );
        let room_size = room.len();

        // Release sessions lock before acquiring documents lock
        drop(sessions);

        // Mark document as having active sessions
        let mut documents = self.documents.write().await;
        if let Some(doc_state) = documents.get_mut(doc_id) {
            doc_state.mark_room_active();
        }
        drop(documents);

        debug!(session_id = %session_id, doc_id = %doc_id, room_size, "Session joined document");

        // Feed the presence registry. The registry deduplicates
        // multi-tab from the same user, so the SSE event only fires
        // on a real "user joined" delta.
        if let DocumentType::Ticket(ticket_id) = doc_type {
            let delta = self
                .presence
                .add_session(user_uuid, ticket_id, session_id.to_string());
            if delta.changed {
                self.emit_viewers_changed(ticket_id).await;
            }
        }
    }

    // Update session activity timestamp
    async fn update_session_activity(&self, doc_id: &str, session_id: &str) {
        let touched: Option<(Uuid, DocumentType)> = {
            let mut sessions = self.sessions.write().await;
            sessions.get_mut(doc_id).and_then(|room| {
                room.get_mut(session_id).map(|info| {
                    info.last_active = Instant::now();
                    (info.user_uuid, info.doc_type)
                })
            })
        };

        // Keep presence's last-active in sync so the avatar stack's
        // recency ordering reflects what the transport sees. No SSE
        // emission: touches never change the viewer set.
        if let Some((user_uuid, DocumentType::Ticket(ticket_id))) = touched {
            self.presence.touch_session(user_uuid, ticket_id);
        }
    }

    /// Record the Yjs `clientID` for a known session so the stale
    /// sweep can later call `Awareness::remove_state` for it. The
    /// id is sniffed from the first awareness frame the client
    /// sends; before that the field is `None` (an unfilled awareness
    /// state on a stale session does no harm — the JS peer's own
    /// 30s outdatedTimeout cleans it up).
    async fn record_yjs_client_id(&self, doc_id: &str, session_id: &str, client_id: u64) {
        let mut sessions = self.sessions.write().await;
        if let Some(room) = sessions.get_mut(doc_id) {
            if let Some(info) = room.get_mut(session_id) {
                info.yjs_client_id = Some(client_id);
            }
        }
    }

    // Remove session
    async fn remove_session(&self, doc_id: &str, session_id: &str) {
        let mut sessions = self.sessions.write().await;

        if let Some(room) = sessions.get_mut(doc_id) {
            let removed = room
                .remove(session_id)
                .map(|info| (info.user_uuid, info.doc_type));
            let room_size = room.len();
            let is_empty = room.is_empty();
            debug!(session_id = %session_id, doc_id = %doc_id, room_size, "Session left document");

            // Release the sessions lock before any async operations
            drop(sessions);

            // Mirror the removal into the presence registry. The
            // registry only reports `changed = true` when this was
            // the user's last tab on the ticket, so multi-tab close
            // doesn't spam the wire.
            if let Some((user_uuid, DocumentType::Ticket(ticket_id))) = removed {
                let delta = self
                    .presence
                    .remove_session(user_uuid, ticket_id, session_id);
                if delta.changed {
                    self.emit_viewers_changed(ticket_id).await;
                }
            }

            // If room is empty, mark it as empty but don't save immediately
            if is_empty {
                debug!(doc_id = %doc_id, "Room is now empty, will save after delay");

                // Mark the document as having an empty room
                let mut documents = self.documents.write().await;
                if let Some(doc_state) = documents.get_mut(doc_id) {
                    doc_state.mark_room_empty();
                }
            }
        }
    }

    // Clean up stale sessions
    async fn cleanup_stale_sessions(&self) {
        let mut sessions = self.sessions.write().await;
        let now = Instant::now();
        let mut stale_session_count = 0;
        let mut newly_empty_rooms = Vec::new();
        // (ticket_id, user_uuid, session_id) tuples to drop from the
        // presence registry after we release the sessions lock.
        let mut presence_drops: Vec<(i32, Uuid, String)> = Vec::new();
        // (doc_id, yjs_client_id) tuples to remove from Yjs awareness
        // after we release the sessions lock. Without this drain,
        // stale clients leave ghost cursors in the room because the
        // sweep used to drop the session row without telling Yjs.
        let mut awareness_drops: Vec<(String, u64)> = Vec::new();

        // First pass: collect stale sessions
        for (doc_id, room) in sessions.iter_mut() {
            let mut stale_sessions = Vec::new();
            let was_empty = room.is_empty();
            // All sessions in a room share the doc, so any one carries
            // the resolved doc_type. Presence is ticket-only.
            let ticket_id = match room.values().next().map(|i| i.doc_type) {
                Some(DocumentType::Ticket(id)) => Some(id),
                _ => None,
            };

            for (session_id, info) in room.iter() {
                if now.duration_since(info.last_active) > *CLIENT_TIMEOUT * 5 {
                    stale_sessions.push((session_id.clone(), info.user_uuid, info.yjs_client_id));
                }
            }

            stale_session_count += stale_sessions.len();

            for (session_id, user_uuid, yjs_client_id) in stale_sessions.iter() {
                debug!(session_id = %session_id, doc_id = %doc_id, "Removing stale session");
                room.remove(session_id);
                if let Some(tid) = ticket_id {
                    presence_drops.push((tid, *user_uuid, session_id.clone()));
                }
                if let Some(cid) = yjs_client_id {
                    awareness_drops.push((doc_id.clone(), *cid));
                }
            }

            // If room just became empty, mark it
            if !was_empty && room.is_empty() {
                newly_empty_rooms.push(doc_id.clone());
            }
        }

        // Log cleanup summary
        if stale_session_count > 0 {
            info!(count = stale_session_count, "Cleaned up stale sessions");
        }

        // Release the sessions lock before updating document states
        drop(sessions);

        // Mark newly empty rooms and drain stale awareness states.
        // Both reach into the documents map; we do them in one lock
        // acquisition so the maintenance loop only pays the lock
        // cost once. Without `remove_state` + the tombstone update
        // broadcast, ghost cursors persist for other viewers until
        // their own peer-side outdatedTimeout (~30s on the JS
        // client; indefinite on yrs peers) cleans them up.
        if !newly_empty_rooms.is_empty() || !awareness_drops.is_empty() {
            let mut documents = self.documents.write().await;
            for doc_id in newly_empty_rooms {
                if let Some(doc_state) = documents.get_mut(&doc_id) {
                    debug!(doc_id = %doc_id, "Marking room empty due to stale session cleanup");
                    doc_state.mark_room_empty();
                }
            }
            // Pull the awareness tombstone updates out under the
            // documents lock; broadcast them after release so a
            // slow per-session channel can't stall other doc
            // operations.
            //
            // Same `remove_state` + `update_with_clients` shape the
            // session_task's disconnect cleanup uses (see the
            // disconnect path near `yjs_client_id` cleanup) —
            // factor out if a third caller ever wants it.
            let mut tombstones: Vec<(String, Vec<u8>)> = Vec::new();
            for (doc_id, client_id) in awareness_drops {
                if let Some(doc_state) = documents.get_mut(&doc_id) {
                    let yrs_client_id = yrs::ClientID::new(client_id);
                    doc_state.awareness.remove_state(yrs_client_id);
                    if let Ok(update) = doc_state.awareness.update_with_clients([yrs_client_id]) {
                        use yrs::sync::Message;
                        let msg = Message::Awareness(update).encode_v1();
                        tombstones.push((doc_id, msg));
                    }
                }
            }
            drop(documents);
            for (doc_id, bytes) in tombstones {
                // Empty sender_id so every session in the room
                // receives it (no self-skip).
                self.broadcast(&doc_id, "", &bytes).await;
            }
        }

        // Mirror drops into the presence registry and emit one
        // `ViewersChanged` per ticket whose viewer set actually
        // changed (i.e. a user lost their last session). Per-ticket
        // dedup means a user closing N tabs in a row produces at
        // most one wire event.
        let mut tickets_to_notify: std::collections::HashSet<i32> =
            std::collections::HashSet::new();
        for (ticket_id, user_uuid, session_id) in presence_drops {
            let delta = self
                .presence
                .remove_session(user_uuid, ticket_id, &session_id);
            if delta.changed {
                tickets_to_notify.insert(ticket_id);
            }
        }
        for ticket_id in tickets_to_notify {
            self.emit_viewers_changed(ticket_id).await;
        }
    }

    // Force save a document immediately and create revision if this is end of editing session
    async fn force_save_document(&self, doc_id: &str) {
        let mut documents = self.documents.write().await;
        if let Some(doc_state) = documents.get_mut(doc_id) {
            let workspace_id = doc_state.workspace_id;
            debug!(doc_id = %doc_id, "Force saving document on disconnect");
            // End-of-session save (last editor left): emit content-saved
            // when the session changed content, so the search replicator
            // re-indexes the body on every machine.
            self.save_document_internal(
                doc_id,
                &doc_state.awareness,
                workspace_id,
                doc_state.fence,
                doc_state.doc_type,
                !doc_state.contributors.is_empty(),
            );
            doc_state.mark_saved();

            // Create revision at end of editing session if there were actual content changes
            // Contributors are only added when content actually changes, so this is sufficient
            if !doc_state.contributors.is_empty() {
                info!(doc_id = %doc_id, contributors = doc_state.contributors.len(),
                    "Creating session-end revision");
                let contributors = doc_state.contributors.clone();
                self.create_snapshot_revision(
                    doc_id,
                    &doc_state.awareness,
                    contributors,
                    workspace_id,
                    doc_state.doc_type,
                );
                doc_state.reset_snapshot_tracking();
            } else {
                debug!(doc_id = %doc_id, "Skipping revision - no content changes in session");
            }

            // Mark final save completed so periodic task doesn't duplicate
            doc_state.mark_final_save_completed();
        }
    }

    /// Flush every document with pending changes to its backing table,
    /// awaiting the writes so they land before the process exits. Bounded
    /// concurrency + an overall `deadline` keep us inside the platform's
    /// shutdown grace window; whatever doesn't finish in time falls back
    /// to the pre-existing best-effort behaviour (the next start reloads
    /// the last durable save). Wired to the SIGTERM/SIGINT handler in
    /// `main.rs` so a Fly deploy no longer drops in-flight edits.
    pub async fn flush_all_dirty(&self, deadline: Duration) {
        // Snapshot the work under the read lock (encode is synchronous),
        // then release the lock before awaiting any DB write.
        let work: Vec<(DocumentType, Vec<u8>, i32, Option<i64>)> = {
            let documents = self.documents.read().await;
            documents
                .iter()
                .filter(|(_, s)| s.has_pending_changes)
                .map(|(_, s)| {
                    (
                        s.doc_type,
                        encode_doc_update(&s.awareness),
                        s.workspace_id,
                        s.fence,
                    )
                })
                .collect()
        };
        if work.is_empty() {
            return;
        }
        let count = work.len();
        info!(
            documents = count,
            "Flushing collaborative documents before shutdown"
        );

        let pool = self.pool.clone();
        let search = self.search_service.clone();
        let flush = futures::stream::iter(work.into_iter().map(
            |(doc_type, content, workspace_id, fence)| {
                let pool = pool.clone();
                let search = search.clone();
                async move {
                    // Shutdown flush: persist the body, but skip the
                    // content-saved emit — a restart rebuilds the index from
                    // Postgres anyway, and the owner is going away.
                    write_yjs_state(pool, search, doc_type, content, workspace_id, fence, false)
                        .await;
                }
            },
        ))
        .buffer_unordered(8)
        .collect::<Vec<()>>();

        match tokio::time::timeout(deadline, flush).await {
            Ok(_) => info!(documents = count, "Shutdown flush complete"),
            Err(_) => warn!(
                deadline_ms = deadline.as_millis() as u64,
                "Shutdown flush hit its deadline; some documents may not have persisted"
            ),
        }
    }

    /// Write a crash-recovery checkpoint for every document that changed
    /// since its last checkpoint. A cheap binary append to `yjs_snapshots`
    /// (no markdown / search / revision work) on a faster cadence than the
    /// heavy `article_contents` save, so a hard crash (SIGKILL / OOM /
    /// panic) loses seconds rather than the whole save interval. The
    /// owning machine is the only one holding a doc in memory, so only it
    /// checkpoints; merge-on-resume makes a stale append harmless, so
    /// (unlike the canonical article_contents save) the append needs no
    /// fence (the table has no fence column by design).
    async fn checkpoint_all_active_documents(&self) {
        // Collect changed docs under the write lock (encode is sync) and
        // advance last_checkpoint_at optimistically; a failed write just
        // means the next edit re-checkpoints. Release the lock before the
        // DB writes.
        let work: Vec<(String, i32, Vec<u8>, Vec<u8>)> = {
            let mut documents = self.documents.write().await;
            documents
                .iter_mut()
                .filter(|(_, s)| s.update_counter > s.last_checkpoint_at)
                .map(|(doc_id, s)| {
                    let (snapshot, state_vector) = encode_doc_full(&s.awareness);
                    s.last_checkpoint_at = s.update_counter;
                    (doc_id.clone(), s.workspace_id, snapshot, state_vector)
                })
                .collect()
        };
        if work.is_empty() {
            return;
        }
        let count = work.len();
        let pool = self.pool.clone();
        futures::stream::iter(work.into_iter().map(
            |(doc_id, workspace_id, snapshot, state_vector)| {
                let pool = pool.clone();
                async move {
                    let mut conn = match pool.get() {
                        Ok(c) => c,
                        Err(e) => {
                            error!(doc_id = %doc_id, error = ?e, "DB conn error writing checkpoint");
                            return;
                        }
                    };
                    let actor = yjs_session_actor(workspace_id);
                    let res = session::with_actor_context(&mut conn, &actor, |conn| {
                        repository::yjs_snapshots::insert_and_prune(
                            conn,
                            workspace_id,
                            &doc_id,
                            &snapshot,
                            &state_vector,
                        )
                    });
                    if let Err(e) = res {
                        error!(doc_id = %doc_id, error = ?e, "Failed to write crash-recovery checkpoint");
                    }
                }
            },
        ))
        .buffer_unordered(8)
        .collect::<Vec<()>>()
        .await;
        debug!(documents = count, "Wrote crash-recovery checkpoints");
    }

    // Broadcast update to all sessions in a room except sender
    async fn broadcast(&self, doc_id: &str, sender_id: &str, msg: &[u8]) {
        if msg.is_empty() {
            return;
        }

        // Collect sender clones while holding the read lock; the
        // mpsc::UnboundedSender is cheap to clone (Arc internally).
        // Cloning under the lock lets us release it before doing the
        // (non-blocking) per-recipient send.
        let recipients: Vec<mpsc::UnboundedSender<Bytes>> = {
            let sessions = self.sessions.read().await;

            if let Some(room) = sessions.get(doc_id) {
                room.iter()
                    .filter(|(id, _)| *id != sender_id)
                    .map(|(_, info)| info.tx.clone())
                    .collect()
            } else {
                Vec::new()
            }
        };

        // `send` on an unbounded mpsc only fails if the receiver was
        // dropped (the session task exited). That can race with our
        // snapshot above; ignore the error and let the next
        // cleanup_stale_sessions sweep evict the dead row.
        let msg_bytes = Bytes::copy_from_slice(msg);
        for tx in recipients {
            let _ = tx.send(msg_bytes.clone());
        }
    }

    // Save document state to the database from awareness
    fn save_document_internal(
        &self,
        doc_id: &str,
        awareness: &Awareness,
        workspace_id: i32,
        fence: Option<i64>,
        doc_type: DocumentType,
        // True only when this is the end-of-session save and the session
        // had real content changes; threaded to `write_yjs_state` to drive
        // the search-only content-saved emit.
        emit_saved: bool,
    ) {
        // Get binary content from the document
        let binary_content = {
            let doc = awareness.doc();
            let txn = doc.transact();

            // DIAGNOSTIC: Show ALL root types in the document
            let root_names: Vec<String> =
                txn.root_refs().map(|(name, _)| name.to_string()).collect();
            trace!(doc_id = %doc_id, root_types = ?root_names, "SAVE - root types");

            // Log state vector to see which clients have contributed
            let state_vec = txn.state_vector();
            trace!(doc_id = %doc_id, state_vector = ?state_vec, "SAVE - state vector");

            // Log content preview before saving
            if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
                let child_count = fragment.len(&txn);
                let preview = safe_get_fragment_string(&fragment, &txn)
                    .map(|s| s.chars().take(50).collect::<String>())
                    .unwrap_or_else(|| "(invalid chars)".to_string());
                debug!(doc_id = %doc_id, child_count, preview = %preview, "Saving document");
            } else {
                warn!(doc_id = %doc_id, "Saving document: NO 'prosemirror' fragment found");
            }

            txn.encode_state_as_update_v1(&StateVector::default())
        };

        debug!(doc_id = %doc_id, bytes = binary_content.len(), "Saving document content");

        // CRITICAL: Save to Redis first (hot cache - survives restarts)
        // This ensures the latest state is always in Redis for fast recovery
        let redis_cache = self.redis_cache.clone();
        let doc_id_clone = doc_id.to_string();
        let content_for_redis = binary_content.clone();
        actix_web::rt::spawn(async move {
            redis_cache
                .set_document(&doc_id_clone, &content_for_redis)
                .await;
            // Also refresh TTL to keep active documents cached longer
            redis_cache.refresh_ttl(&doc_id_clone).await;
        });

        // Persist to the backing table via the shared awaitable writer.
        // Spawned here (fire-and-forget) so the periodic / on-disconnect
        // save never blocks the maintenance loop; `flush_all_dirty` awaits
        // the same writer on shutdown. One write path, one fence + RLS
        // contract (DRY).
        actix_web::rt::spawn(write_yjs_state(
            self.pool.clone(),
            self.search_service.clone(),
            doc_type,
            binary_content,
            workspace_id,
            fence,
            emit_saved,
        ));
    }

    // Create a snapshot revision for version history using native Yrs encoding
    fn create_snapshot_revision(
        &self,
        doc_id: &str,
        awareness: &Awareness,
        contributors: HashSet<Uuid>,
        workspace_id: i32,
        doc_type: DocumentType,
    ) {
        // Encode document state using native Yrs functions
        let (state_vector_bytes, full_update_bytes) = {
            let doc = awareness.doc();
            let txn = doc.transact();

            // Use Yrs native encoding
            let state_vector = txn.state_vector();
            let full_update = txn.encode_state_as_update_v1(&StateVector::default());

            (state_vector.encode_v1(), full_update)
        };

        debug!(doc_id = %doc_id, bytes = full_update_bytes.len(), "Creating snapshot");

        // Save to database asynchronously
        let pool = self.pool.clone();
        let contributor_vec: Vec<Option<Uuid>> = contributors.into_iter().map(Some).collect();

        match doc_type {
            DocumentType::Ticket(ticket_id) => {
                let contributor_count = contributor_vec.len();
                let actor = yjs_session_actor(workspace_id);
                actix_web::rt::spawn(async move {
                    // Six interleaved repo calls against RLS-enabled
                    // tables (article_contents, article_content_revisions,
                    // tickets). Run them in one workspace-pinned txn
                    // so they share one elevation, RLS enforces the
                    // workspace boundary on every write, and a
                    // partial failure rolls back cleanly.
                    let outcome = match pool.get() {
                        Ok(mut conn) => crate::sync::session::with_actor_context::<
                            _,
                            diesel::result::Error,
                        >(&mut conn, &actor, |conn| {
                            // Get or create article_content record.
                            let article_content =
                                match repository::get_article_content_by_ticket_id(conn, ticket_id)
                                {
                                    Ok(ac) => ac,
                                    Err(_) => {
                                        let new_content = NewArticleContent {
                                            ticket_id,
                                            yjs_state_vector: None,
                                            yjs_document: None,
                                            yjs_client_id: None,
                                        };
                                        repository::create_article_content(conn, new_content)?
                                    }
                                };

                            // Skip if content matches the last revision.
                            if let Ok(last_revision) =
                                repository::get_latest_article_content_revision(
                                    conn,
                                    article_content.id,
                                )
                            {
                                if last_revision.yjs_document_content == full_update_bytes {
                                    debug!(
                                        ticket_id,
                                        revision = last_revision.revision_number,
                                        "Skipping revision - content unchanged"
                                    );
                                    return Ok(None);
                                }
                            }

                            let new_revision = NewArticleContentRevision {
                                article_content_id: article_content.id,
                                revision_number: article_content.current_revision_number,
                                yjs_state_vector: state_vector_bytes,
                                yjs_document_content: full_update_bytes,
                                contributed_by: contributor_vec,
                            };

                            let revision =
                                repository::create_article_content_revision(conn, new_revision)?;
                            repository::increment_article_content_revision(
                                conn,
                                article_content.id,
                            )?;
                            // Best-effort: a failure here is logged
                            // but doesn't abort the snapshot.
                            if let Err(e) =
                                repository::update_ticket_modified_timestamp(conn, ticket_id)
                            {
                                warn!(ticket_id, error = ?e, "Failed to update ticket modified timestamp");
                            }
                            Ok(Some(revision.revision_number))
                        }),
                        Err(e) => {
                            error!(ticket_id, error = %e, "Database connection error during snapshot");
                            return;
                        }
                    };

                    match outcome {
                        Ok(Some(rev_num)) => info!(
                            ticket_id,
                            revision = rev_num,
                            contributors = contributor_count,
                            "Snapshot created for ticket"
                        ),
                        Ok(None) => {} // Skip already logged inside the closure.
                        Err(e) => {
                            error!(ticket_id, error = %e, "Snapshot creation failed for ticket")
                        }
                    }
                });
            }
            DocumentType::Documentation(doc_page_id) => {
                let contributor_count = contributor_vec.len();
                let actor = yjs_session_actor(workspace_id);
                actix_web::rt::spawn(async move {
                    let outcome = match pool.get() {
                        Ok(mut conn) => crate::sync::session::with_actor_context::<
                            _,
                            diesel::result::Error,
                        >(&mut conn, &actor, |conn| {
                            if let Ok(last_revision) =
                                repository::get_latest_documentation_revision(conn, doc_page_id)
                            {
                                if last_revision.yjs_document_snapshot == full_update_bytes {
                                    debug!(
                                        doc_page_id,
                                        revision = last_revision.revision_number,
                                        "Skipping revision - content unchanged"
                                    );
                                    return Ok(None);
                                }
                            }

                            let revision_number = repository::create_documentation_revision(
                                conn,
                                doc_page_id,
                                state_vector_bytes,
                                full_update_bytes,
                                contributor_vec,
                            )?;
                            Ok(Some(revision_number))
                        }),
                        Err(e) => {
                            error!(doc_page_id, error = %e, "Database connection error during snapshot");
                            return;
                        }
                    };

                    match outcome {
                        Ok(Some(rev_num)) => info!(
                            doc_page_id,
                            revision = rev_num,
                            contributors = contributor_count,
                            "Snapshot created for documentation page"
                        ),
                        Ok(None) => {}
                        Err(e) => {
                            error!(doc_page_id, error = %e, "Snapshot creation failed for documentation page")
                        }
                    }
                });
            }
            DocumentType::Collection(collection_id) => {
                // Collections don't have a per-revision history
                // table yet — the live save in `save_document_internal`
                // already lands the latest state. Skip the snapshot
                // pass without warning. If revision history is added
                // later, mirror the documentation pattern above.
                let _ = (
                    collection_id,
                    contributor_vec,
                    state_vector_bytes,
                    full_update_bytes,
                );
                debug!(
                    collection_id,
                    "Collection description snapshot skipped (no revision history)"
                );
            }
        }
    }
}

// =================================================================
// WebSocket session task (actix-ws, ex-actor implementation).
//
// One task per connection. Owns the `Session` (write side) and
// drains both the inbound `AggregatedMessageStream` and a per-
// session `mpsc::UnboundedReceiver<Bytes>` that other sessions
// push broadcast / awareness frames into via the YjsAppState
// session map. tokio::select! multiplexes inbound + outbound +
// heartbeat tick in a single loop, replacing the actor's
// Stream/Handler/run_interval triplet.
// =================================================================

/// WebSocket connection handler — entry point for WebSocket requests.
pub async fn ws_handler(
    req: HttpRequest,
    body: web::Payload,
    app_state: web::Data<YjsAppState>,
    ws: crate::extractors::WorkspaceContext,
    path: web::Path<String>,
) -> Result<HttpResponse, Error> {
    let doc_id = path.into_inner();
    debug!(doc_id = %doc_id, "WebSocket connection request");

    // Validate Origin header to prevent WebSocket hijacking (CSWSH).
    // Trust the same CorsAllowlist the HTTP CORS layer enforces
    // (FRONTEND_URL + ADDITIONAL_CORS_ORIGINS + any tenant-subdomain
    // regex). Any origin the operator has allowed for credentialed API
    // calls can also open the collab socket; pinning this to
    // FRONTEND_URL alone silently broke realtime editing for
    // multi-origin deployments (e.g. reaching the app by LAN IP and by
    // hostname). This widens nothing security-wise: the allowlist
    // can't trust an origin that isn't already trusted for credentialed
    // HTTP.
    let is_production = std::env::var("ENVIRONMENT")
        .map(|v| v.to_lowercase() == "production")
        .unwrap_or(false);

    match req.headers().get("Origin") {
        Some(origin) => {
            let origin_str = origin.to_str().unwrap_or("");
            let origin_normalized = origin_str.trim_end_matches('/');
            // A same-origin WebSocket (the Origin's authority equals the
            // Host this request was sent to) is by definition not a
            // cross-site request, so it can't be CSWSH. In development we
            // accept it, which lets the dev stack be reached over any host
            // (e.g. the machine's LAN IP when testing from another device)
            // without listing it. Production requires an allowlisted
            // origin.
            let same_origin = !is_production && {
                let host = req
                    .headers()
                    .get("Host")
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("");
                let origin_authority = origin_normalized
                    .split_once("://")
                    .map(|(_, authority)| authority)
                    .unwrap_or("");
                !host.is_empty() && origin_authority == host
            };
            if !crate::utils::cors_allowlist::global().allows(origin_normalized) && !same_origin {
                warn!(origin = %origin_str, "WebSocket origin not in CORS allowlist");
                return Err(actix_web::error::ErrorForbidden("Invalid origin"));
            }
        }
        None => {
            // Origin should always be present from browsers (per spec)
            // In production, require it; in dev, allow for testing tools
            if is_production {
                warn!("WebSocket request missing Origin header in production");
                return Err(actix_web::error::ErrorForbidden("Origin header required"));
            }
            debug!("WebSocket request without Origin header (allowed in non-production)");
        }
    }

    // Extract and validate JWT token from httpOnly cookie
    let token = req
        .cookie(crate::utils::cookies::ACCESS_TOKEN_COOKIE)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No authentication cookie"))?;

    // Validate the token, extract the user UUID, and resolve the
    // visibility context used to gate per-document access below.
    let (user_uuid, accessor) = if let Some(pool) = req.app_data::<web::Data<crate::db::Pool>>() {
        let mut conn = pool.get().map_err(|_| {
            actix_web::error::ErrorInternalServerError("Database connection failed")
        })?;

        // Use our centralized JWT validation
        use crate::utils::jwt::JwtUtils;

        match JwtUtils::validate_token_with_user_check(token.value(), &mut conn).await {
            Ok((claims, user)) => {
                let accessor = DocAccessor::from_claims(&claims, &mut conn)
                    .ok_or_else(|| actix_web::error::ErrorUnauthorized("Invalid token subject"))?;
                (user.uuid, accessor)
            }
            Err(_) => {
                return Err(actix_web::error::ErrorUnauthorized(
                    "Invalid or expired token",
                ))
            }
        }
    } else {
        return Err(actix_web::error::ErrorInternalServerError(
            "Database pool not available",
        ));
    };

    // Parse the workspace-namespaced doc_id and validate that its
    // claimed workspace matches the WorkspaceContext the middleware
    // resolved from this request. This is the cache-key + tenancy
    // guard:
    //
    //   * Rejects legacy bare ids ("ticket-N"), so a stale client
    //     that didn't refresh after the namespace migration fails
    //     fast with a clear log line instead of silently sharing
    //     the workspace-1 doc.
    //   * Rejects cross-workspace requests under hosted multi-
    //     tenancy: a tab cached against workspace A whose state
    //     somehow constructs a `ws-{A_uuid}_...` docId after
    //     navigating to workspace B can't accidentally read or
    //     write workspace B's doc.
    let parsed = match DocumentType::from_namespaced_doc_id(&doc_id) {
        Ok(p) => p,
        Err(e) => {
            warn!(doc_id = %doc_id, error = ?e, "WebSocket doc_id parse failed");
            return Err(actix_web::error::ErrorBadRequest(
                "doc_id must be in the workspace-namespaced format ws-{uuid}_{kind}-{id}",
            ));
        }
    };
    if parsed.workspace_uuid != ws.workspace_uuid {
        warn!(
            doc_id = %doc_id,
            requested_workspace_uuid = %parsed.workspace_uuid,
            request_workspace_id = ws.workspace_id,
            "WebSocket doc_id workspace mismatch — rejecting cross-tenant request"
        );
        return Err(actix_web::error::ErrorForbidden(
            "doc_id workspace does not match the request workspace",
        ));
    }
    debug!(
        doc_id = %doc_id,
        user_uuid = %user_uuid,
        workspace_id = ws.workspace_id,
        "WebSocket authentication + workspace check successful"
    );

    // Resolve the doc_id's immutable resource UUID to the integer-keyed
    // document type, then run the per-document visibility gate. The
    // workspace check above bounds the tenant; this bounds which
    // documents *within* the workspace the caller may open. Without it a
    // restricted member who cannot read ticket N via REST/SSE could
    // still read and write its Yjs note here. See security-audit-2026-06.
    // The resolved doc_type is threaded into the session so the save /
    // presence paths never re-parse the doc_id.
    let doc_type = {
        let pool = req
            .app_data::<web::Data<crate::db::Pool>>()
            .ok_or_else(|| {
                actix_web::error::ErrorInternalServerError("Database pool not available")
            })?;
        let mut conn = pool.get().map_err(|_| {
            actix_web::error::ErrorInternalServerError("Database connection failed")
        })?;
        // Resolve the resource + run the visibility gate under the request's
        // workspace context. `parsed.resolve` reads `tickets`/`documentation`
        // and `can_access_document` reads more tenant tables — all RLS-scoped
        // by `app.workspace_id`. On a raw connection that GUC is unset, so the
        // rows are filtered out and every doc 404s in hosted mode. The rest of
        // this handler already wraps its reads this way.
        let actor = yjs_session_actor(ws.workspace_id);
        let resolved = session::with_actor_context(&mut conn, &actor, |conn| {
            let dt = match parsed.resolve(conn)? {
                Some(dt) => dt,
                None => return Ok(None),
            };
            let allowed = can_access_document(conn, &accessor, &dt)?;
            Ok::<_, diesel::result::Error>(Some((dt, allowed)))
        });
        match resolved {
            Ok(Some((dt, true))) => dt,
            Ok(Some((_, false))) => {
                warn!(doc_id = %doc_id, user_uuid = %user_uuid, "WebSocket document access denied");
                return Err(actix_web::error::ErrorNotFound("Document not found"));
            }
            Ok(None) => {
                warn!(doc_id = %doc_id, "WebSocket doc_id resolves to no live resource");
                return Err(actix_web::error::ErrorNotFound("Document not found"));
            }
            Err(e) => {
                error!(doc_id = %doc_id, error = ?e, "WebSocket doc resolution/visibility check failed");
                return Err(actix_web::error::ErrorInternalServerError(
                    "Access check failed",
                ));
            }
        }
    };

    // Per-document affinity routing (Phase 2). In single-instance mode
    // this is always `Local`. Under multi-instance routing, if another
    // machine owns this doc we return a `fly-replay` response WITHOUT
    // negotiating the upgrade, so fly-proxy replays the original request
    // to the owning machine, which then performs the upgrade. This is
    // the one constraint the research surfaced: the replaying instance
    // must not handle the upgrade itself. See
    // `docs/realtime-collab-affinity-design.md`.
    let fence = match app_state.route(&doc_id).await {
        CollabRoute::Local(fence) => fence,
        CollabRoute::ReplayTo(owner) => {
            debug!(doc_id = %doc_id, owner = %owner, "Replaying WebSocket to owning machine");
            return Ok(HttpResponse::Ok()
                .insert_header(("fly-replay", format!("instance={owner}")))
                .finish());
        }
        CollabRoute::Rehandshake => {
            // Direct-address mode: this machine isn't the owner. Tell the
            // client to re-run the handshake to learn the owner's address.
            debug!(doc_id = %doc_id, "WS landed on non-owner; instructing client to re-handshake");
            return Ok(HttpResponse::Conflict().json(json!({
                "error": "rehandshake_required",
                "handshake": format!("/api/collaboration/handshake/{doc_id}"),
            })));
        }
    };

    // Hand off to actix-ws: returns (HttpResponse, Session, MessageStream).
    // The response is returned to the framework synchronously so the
    // 101 Upgrade lands; the session_task runs detached and owns the
    // socket for its lifetime.
    let (response, session, msg_stream) = actix_ws::handle(&req, body)?;

    // 1 MiB frame limit (matches the old WsResponseBuilder.frame_size()).
    // Yjs documents with history can exceed the 64 KiB default.
    // `aggregate_continuations` collapses fragmented frames into single
    // AggregatedMessage::{Binary,Text} so process_message sees one whole
    // payload, matching the old StreamHandler semantics.
    let msg_stream = msg_stream
        .max_frame_size(1024 * 1024)
        .aggregate_continuations()
        .max_continuation_size(1024 * 1024);

    let session_id = Uuid::now_v7().to_string();
    let workspace_id = ws.workspace_id;
    let app_state_inner = app_state.get_ref().clone();

    actix_web::rt::spawn(session_task(
        session_id,
        doc_id,
        app_state_inner,
        user_uuid,
        workspace_id,
        fence,
        doc_type,
        session,
        msg_stream,
    ));

    Ok(response)
}

/// Per-connection async task. Replaces the Actor + StreamHandler +
/// Handler<YjsMessage> triplet from the actix-web-actors era.
async fn session_task(
    session_id: String,
    doc_id: String,
    app_state: YjsAppState,
    user_uuid: Uuid,
    workspace_id: i32,
    fence: Option<i64>,
    doc_type: DocumentType,
    mut session: actix_ws::Session,
    mut msg_stream: actix_ws::AggregatedMessageStream,
) {
    let started_at = Instant::now();
    let mut last_hb = started_at;
    let mut messages_received: u32 = 0;
    let mut pings_sent: u32 = 0;
    let mut pongs_received: u32 = 0;
    let mut yjs_client_id: Option<u64> = None;

    info!(
        session_id = %session_id, doc_id = %doc_id,
        heartbeat_interval_secs = HEARTBEAT_INTERVAL.as_secs(),
        timeout_secs = (*CLIENT_TIMEOUT + Duration::from_secs(30)).as_secs(),
        "WebSocket STARTED"
    );

    // Per-session outbound channel. The write side lives in the
    // YjsAppState session map (so `broadcast` can target this
    // connection); the read side is drained by the loop below and
    // forwarded to the wire. Unbounded because (a) sends are small,
    // (b) sender-side backpressure would block other sessions'
    // broadcasts, which is worse than letting one slow consumer's
    // backlog grow.
    let (tx, mut rx) = mpsc::unbounded_channel::<Bytes>();
    // Eviction signal: the owning machine notifies this when it drops the
    // document (lost ownership lease, Phase 2 affinity), so this task
    // tears down and the client reconnects to the new owner.
    let cancel = Arc::new(Notify::new());
    app_state
        .register_session(
            &doc_id,
            &session_id,
            tx.clone(),
            user_uuid,
            cancel.clone(),
            doc_type,
        )
        .await;

    // Per the yjs sync protocol spec, the server proactively sends
    // SyncStep1 + all known awareness states to newly-connected
    // clients. Each message goes as its own frame because
    // y-websocket's readMessage() parses one message per frame —
    // packing them would lose all but the first.
    {
        let awareness = app_state
            .get_or_create_awareness(&doc_id, workspace_id, fence, doc_type)
            .await;
        use yrs::sync::{Message, SyncMessage};

        let sv = awareness.doc().transact().state_vector();
        let sync_msg = Message::Sync(SyncMessage::SyncStep1(sv));
        let _ = tx.send(Bytes::from(sync_msg.encode_v1()));

        match awareness.update() {
            Ok(awareness_update) => {
                let awareness_msg = Message::Awareness(awareness_update);
                let _ = tx.send(Bytes::from(awareness_msg.encode_v1()));
                debug!(doc_id = %doc_id, "Sent initial SyncStep1 + awareness to new client");
            }
            Err(e) => {
                debug!(doc_id = %doc_id, error = ?e,
                    "Sent SyncStep1 but no awareness states to send");
            }
        }
    }

    let mut heartbeat = tokio::time::interval(*HEARTBEAT_INTERVAL);
    // First tick fires immediately; skip it so we don't ping before the
    // client has even drained the initial sync.
    heartbeat.tick().await;

    // Set when this task exits because the document was evicted
    // (ownership handoff), so the disconnect cleanup below skips the
    // awareness-tombstone + final-save work that would resurrect the
    // just-evicted doc into memory.
    let mut evicted = false;
    let close_reason: Option<CloseReason> = loop {
        tokio::select! {
            // Eviction: the owning machine dropped this document. Tear
            // down without touching the (already-removed) doc state.
            _ = cancel.notified() => {
                debug!(session_id = %session_id, doc_id = %doc_id,
                    "Session evicted for ownership handoff; closing");
                evicted = true;
                break None;
            }
            // Outbound: broadcast payloads from `app_state.broadcast`,
            // self-generated protocol responses, awareness updates, etc.
            // `rx.recv()` returns None when every Sender clone is dropped,
            // which only happens during cleanup. Send failures (peer hung
            // up mid-flight) collapse the loop.
            Some(out) = rx.recv() => {
                if session.binary(out).await.is_err() {
                    debug!(session_id = %session_id, "session.binary failed; client gone");
                    break None;
                }
            }
            // Inbound frame from the client.
            msg = msg_stream.next() => match msg {
                Some(Ok(AggregatedMessage::Ping(payload))) => {
                    trace!(session_id = %session_id, "WebSocket received PING");
                    last_hb = Instant::now();
                    messages_received += 1;
                    if session.pong(&payload).await.is_err() {
                        break None;
                    }
                }
                Some(Ok(AggregatedMessage::Pong(_))) => {
                    trace!(session_id = %session_id, "WebSocket received PONG");
                    last_hb = Instant::now();
                    pongs_received += 1;
                    messages_received += 1;
                }
                Some(Ok(AggregatedMessage::Binary(bin))) => {
                    trace!(session_id = %session_id, bytes = bin.len(),
                        "WebSocket received BINARY message");
                    last_hb = Instant::now();
                    messages_received += 1;

                    // Capture the yjs clientID from the first awareness
                    // message (msg type 1). Needed to clean up the
                    // awareness state on disconnect AND to make
                    // `cleanup_stale_sessions` able to call
                    // `Awareness::remove_state` for stale clients
                    // (it reads the id from SessionInfo).
                    if yjs_client_id.is_none()
                        && bin.first() == Some(&1)
                        && bin.len() > 1
                    {
                        use yrs::encoding::read::Cursor;
                        use yrs::sync::AwarenessUpdate;
                        use yrs::updates::decoder::DecoderV1 as ADecV1;
                        if let Ok(update) =
                            AwarenessUpdate::decode(&mut ADecV1::new(Cursor::new(&bin[1..])))
                        {
                            if let Some(&client_id) = update.clients.keys().next() {
                                let client_id_u64 = client_id.get();
                                yjs_client_id = Some(client_id_u64);
                                // Mirror into the session map so the
                                // stale-sweep can find it without
                                // looking inside our task frame.
                                app_state
                                    .record_yjs_client_id(&doc_id, &session_id, client_id_u64)
                                    .await;
                                debug!(session_id = %session_id, yjs_client_id = client_id_u64,
                                    "Captured yjs clientID from awareness");
                            }
                        }
                    }

                    // Process the Yjs message off the loop so heartbeat
                    // ticks and outbound broadcasts to this session
                    // don't stall on protocol work. Same shape as the
                    // old `actix::spawn` inside process_message.
                    let app_state_c = app_state.clone();
                    let doc_id_c = doc_id.clone();
                    let session_id_c = session_id.clone();
                    let tx_c = tx.clone();
                    actix_web::rt::spawn(process_inbound_binary(
                        bin,
                        app_state_c,
                        doc_id_c,
                        session_id_c,
                        user_uuid,
                        tx_c,
                    ));
                }
                Some(Ok(AggregatedMessage::Text(text))) => {
                    warn!(session_id = %session_id, text = %text,
                        "WebSocket received unexpected TEXT message");
                }
                Some(Ok(AggregatedMessage::Close(reason))) => {
                    debug!(session_id = %session_id, reason = ?reason,
                        "WebSocket received CLOSE message");
                    break reason;
                }
                Some(Err(e)) => {
                    error!(session_id = %session_id, error = ?e, "WebSocket protocol error");
                    break None;
                }
                None => {
                    debug!(session_id = %session_id, "Inbound stream ended");
                    break None;
                }
            },
            // Heartbeat tick: send a PING and check for client timeout.
            _ = heartbeat.tick() => {
                let idle = Instant::now().duration_since(last_hb);
                trace!(session_id = %session_id, idle_secs = idle.as_secs(),
                    "WebSocket heartbeat check");

                // Grace period: warn at CLIENT_TIMEOUT, disconnect at + 30s
                if idle > *CLIENT_TIMEOUT + Duration::from_secs(30) {
                    warn!(session_id = %session_id, idle_secs = idle.as_secs(),
                        "WebSocket Client heartbeat TIMEOUT, disconnecting");
                    break None;
                }

                trace!(session_id = %session_id, ping_num = pings_sent + 1,
                    idle_secs = idle.as_secs(), "WebSocket sending PING");
                pings_sent += 1;
                if session.ping(b"").await.is_err() {
                    debug!(session_id = %session_id, "session.ping failed; client gone");
                    break None;
                }

                if idle > *CLIENT_TIMEOUT {
                    warn!(session_id = %session_id, idle_secs = idle.as_secs(),
                        "WebSocket Client heartbeat WARNING");
                }
            }
            else => break None,
        }
    };

    let connection_duration = Instant::now().duration_since(started_at);
    let idle = Instant::now().duration_since(last_hb);
    info!(
        session_id = %session_id, doc_id = %doc_id,
        connection_duration_secs = connection_duration.as_secs(),
        idle_secs = idle.as_secs(),
        messages_received, pings_sent, pongs_received,
        "WebSocket STOPPING"
    );

    // Cleanup mirrors the old Actor::stopping logic. Drop the
    // outbound Sender so `broadcast` invocations from other tasks
    // don't lodge a Bytes into a channel whose receiver is gone.
    drop(tx);

    // When the document was evicted (ownership handoff), its in-memory
    // state and session room are already gone. Skip the normal cleanup:
    // `get_or_create_awareness` would rehydrate (resurrect) the doc this
    // machine no longer owns, and a final save would race the new owner
    // (the fence protects the snapshot regardless). Just close the wire.
    if !evicted {
        app_state.remove_session(&doc_id, &session_id).await;

        // Clean up the disconnected client's awareness state and notify
        // remaining clients. On abrupt disconnects (refresh, network loss)
        // the client can't send this itself, so the server must. Use a
        // non-creating lookup: if the doc was evicted concurrently there
        // is nothing to clean up (and we must not resurrect it).
        if let Some(client_id) = yjs_client_id {
            if let Some(awareness) = app_state.get_awareness(&doc_id).await {
                let yrs_client_id = yrs::ClientID::new(client_id);
                awareness.remove_state(yrs_client_id);

                if let Ok(update) = awareness.update_with_clients([yrs_client_id]) {
                    use yrs::sync::Message;
                    let msg = Message::Awareness(update).encode_v1();
                    app_state.broadcast(&doc_id, &session_id, &msg).await;
                    debug!(doc_id = %doc_id, yjs_client_id = client_id,
                        "Removed awareness state and notified remaining clients");
                }
            }
        }

        // Force-save when this was the last session for the document.
        let should_force_save = {
            let sessions = app_state.sessions.read().await;
            sessions
                .get(&doc_id)
                .map(|room| room.is_empty())
                .unwrap_or(true)
        };
        if should_force_save {
            debug!(doc_id = %doc_id, "Last session for document, performing final save");
            app_state.force_save_document(&doc_id).await;
        }
    }

    let _ = session.close(close_reason).await;
}

/// Process a single inbound binary frame from a client. Runs in a
/// spawned task so the per-session loop stays responsive to
/// heartbeat ticks and outbound broadcasts while this DB / protocol
/// work happens. Self-replies (sync responses, fallback SyncStep1
/// requests) flow back through `tx` rather than directly through
/// the `Session`, so the session_task's outbound arm orders them
/// against other broadcasts the same way it always did.
async fn process_inbound_binary(
    bin: Bytes,
    app_state: YjsAppState,
    doc_id: String,
    session_id: String,
    user_uuid: Uuid,
    tx: mpsc::UnboundedSender<Bytes>,
) {
    if bin.is_empty() {
        return;
    }

    let is_sync_message = bin.first() == Some(&0); // MESSAGE_SYNC

    app_state
        .update_session_activity(&doc_id, &session_id)
        .await;

    // Non-creating lookup: the doc was created at session start. If it's
    // gone (evicted for an ownership handoff), drop the frame rather than
    // resurrecting an unowned doc that would write to storage unfenced.
    let Some(awareness) = app_state.get_awareness(&doc_id).await else {
        debug!(doc_id = %doc_id, "Dropping inbound frame: document no longer resident (evicted)");
        return;
    };

    // Diagnostic: check fragment text BEFORE protocol.handle so we can
    // detect "content actually changed" precisely (some sync messages
    // are no-ops).
    let content_before = {
        let txn = awareness.doc().transact();
        if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
            fragment.get_string(&txn)
        } else {
            String::from("(no fragment)")
        }
    };

    let protocol = DefaultProtocol;
    let msg_type = bin.first().copied().unwrap_or(255);
    trace!(msg_type, bytes = bin.len(), "Processing message");

    if msg_type == 0 && bin.len() > 1 {
        let sync_step = bin[1];
        match sync_step {
            0 => trace!("SYNC_STEP_1 (state vector request)"),
            1 => trace!("SYNC_STEP_2 (state response)"),
            2 => trace!(bytes = bin.len() - 2, "SYNC_UPDATE (incremental change)"),
            _ => trace!(sync_step, "Unknown sync step"),
        }
    }

    match protocol.handle(&awareness, &bin) {
        Ok(messages) => {
            trace!(
                response_count = messages.len(),
                "protocol.handle() succeeded"
            );

            let content_after = {
                let txn = awareness.doc().transact();
                if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
                    fragment.get_string(&txn)
                } else {
                    String::from("(no fragment)")
                }
            };

            let content_changed = content_before != content_after;
            if content_changed {
                debug!(before = %crate::utils::utf8_trunc::char_prefix(&content_before, 50),
                    after = %crate::utils::utf8_trunc::char_prefix(&content_after, 50),
                    "Content changed");
            } else if msg_type == 0 && bin.len() > 1 && bin[1] == 2 {
                // SYNC_UPDATE didn't apply — request the client's full
                // state. Happens when state vectors are misaligned
                // (e.g. after server restart).
                debug!("SYNC_UPDATE did not change content - requesting client's full state");
                use yrs::sync::Message;
                let sync_message =
                    Message::Sync(yrs::sync::SyncMessage::SyncStep1(StateVector::default()));
                let _ = tx.send(Bytes::from(sync_message.encode_v1()));
            }

            for message in messages {
                let encoded = message.encode_v1();
                let _ = tx.send(Bytes::from(encoded));
            }

            // Decide which inbound frames to rebroadcast to other
            // clients in the room. The previous implementation
            // unconditionally rebroadcasted every successfully-
            // handled frame, including SyncStep1 (client → server
            // "what's your state vector?"). Each peer's yrs
            // protocol handler then replied to the originator's
            // claimed room, generating a fanned-out cascade of
            // unnecessary state-vector traffic.
            //
            // The reference y-websocket server only fans out
            // SyncStep2 / SyncUpdate (real data) and Awareness
            // (presence). SyncStep1 is point-to-point and stays
            // between the requesting client and the server. Narrow
            // the broadcast to those frames.
            let should_broadcast = match msg_type {
                // Awareness: always fan out so peers see presence
                // changes.
                1 => true,
                // Sync envelope: only SyncStep2 (subtype 1) and
                // SyncUpdate (subtype 2). SyncStep1 (subtype 0) is
                // a point-to-point state-vector request and must
                // not be rebroadcast.
                0 => {
                    let sync_step = bin.get(1).copied();
                    matches!(sync_step, Some(1) | Some(2))
                }
                // Unknown envelopes (queryAwareness, auth, etc.):
                // don't rebroadcast.
                _ => false,
            };
            if should_broadcast {
                app_state.broadcast(&doc_id, &session_id, &bin).await;
            }

            if is_sync_message || content_changed {
                app_state.mark_document_changed(&doc_id).await;
            }
            if content_changed {
                app_state.add_contributor(&doc_id, user_uuid).await;
            }
        }
        Err(e) => {
            error!(error = ?e, "Error handling protocol message");
        }
    }
}

// ============= Revision History API Endpoints =============

/// Fetch the `article_content` row backing a ticket's collaborative
/// document. `gate_ticket` must have passed first, so a missing row is
/// not an access failure: it just means the ticket's editor was never
/// saved, which is zero revisions (`Ok(None)`). A real query failure
/// maps to a 500 response the caller can return directly.
fn ticket_article_content(
    tc: &mut TenantConn,
    ticket_id: i32,
) -> Result<Option<crate::models::ArticleContent>, HttpResponse> {
    match tc.run(|conn| {
        crate::repository::article_content::get_article_content_by_ticket_id(conn, ticket_id)
    }) {
        Ok(content) => Ok(Some(content)),
        Err(diesel::result::Error::NotFound) => Ok(None),
        Err(e) => {
            error!(ticket_id, error = ?e, "Error loading article content");
            Err(errors::internal("Error retrieving revisions"))
        }
    }
}

/// GET /tickets/:id/revisions - List all revisions for a ticket
pub async fn get_ticket_revisions(
    ticket_id: web::Path<i32>,
    mut tc: TenantConn,
    auth: AuthContext,
) -> HttpResponse {
    let ticket_id = ticket_id.into_inner();
    if let Err(resp) = gate_ticket(&mut tc, &auth, ticket_id) {
        return resp;
    }

    // A ticket with no saved collaborative content yet simply has no
    // revisions: return an empty list so the version-history panel shows
    // its empty state instead of erroring.
    let article_content = match ticket_article_content(&mut tc, ticket_id) {
        Ok(Some(content)) => content,
        Ok(None) => {
            return HttpResponse::Ok()
                .json(Vec::<crate::models::ArticleContentRevisionResponse>::new());
        }
        Err(resp) => return resp,
    };

    // Get all revisions
    match tc.run(|conn| {
        crate::repository::article_content::get_article_content_revisions(conn, article_content.id)
    }) {
        Ok(revisions) => {
            let responses: Vec<crate::models::ArticleContentRevisionResponse> =
                revisions.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(_) => errors::internal("Error retrieving revisions"),
    }
}

/// GET /tickets/:id/revisions/:revision_number - Get a specific revision
pub async fn get_ticket_revision(
    path: web::Path<(i32, i32)>,
    mut tc: TenantConn,
    auth: AuthContext,
) -> HttpResponse {
    let (ticket_id, revision_number) = path.into_inner();
    if let Err(resp) = gate_ticket(&mut tc, &auth, ticket_id) {
        return resp;
    }

    // No saved content means this revision can't exist.
    let article_content = match ticket_article_content(&mut tc, ticket_id) {
        Ok(Some(content)) => content,
        Ok(None) => return errors::not_found_msg("Revision not found"),
        Err(resp) => return resp,
    };

    // Get the specific revision
    match tc.run(|conn| {
        crate::repository::article_content::get_article_content_revision(
            conn,
            article_content.id,
            revision_number,
        )
    }) {
        Ok(revision) => {
            // Encode the Yjs document content as base64 for frontend
            let content_base64 = general_purpose::STANDARD.encode(&revision.yjs_document_content);

            HttpResponse::Ok().json(serde_json::json!({
                "id": revision.id,
                "article_content_id": revision.article_content_id,
                "revision_number": revision.revision_number,
                "yjs_document_content": content_base64,
                "contributed_by": revision.contributed_by,
                "created_at": revision.created_at,
            }))
        }
        Err(_) => errors::not_found_msg("Revision not found"),
    }
}

/// POST /tickets/:id/restore/:revision_number - Restore ticket to a specific revision
pub async fn restore_ticket_revision(
    path: web::Path<(i32, i32)>,
    mut tc: TenantConn,
    ws: crate::extractors::WorkspaceContext,
    app_state: web::Data<YjsAppState>,
    auth: AuthContext,
) -> HttpResponse {
    let (ticket_id, revision_number) = path.into_inner();
    if let Err(resp) = gate_ticket(&mut tc, &auth, ticket_id) {
        return resp;
    }

    // No saved content means this revision can't exist.
    let article_content = match ticket_article_content(&mut tc, ticket_id) {
        Ok(Some(content)) => content,
        Ok(None) => return errors::not_found_msg("Revision not found"),
        Err(resp) => return resp,
    };

    // Get the revision to restore
    let revision = match tc.run(|conn| {
        crate::repository::article_content::get_article_content_revision(
            conn,
            article_content.id,
            revision_number,
        )
    }) {
        Ok(rev) => rev,
        Err(_) => return errors::not_found_msg("Revision not found"),
    };

    // Build the same workspace-namespaced, UUID-keyed doc_id the clients
    // connect with, so the restore targets the live room (not a phantom
    // integer-keyed doc no session is attached to).
    let doc_id = match tc.run(|conn| crate::repository::tickets::uuid_by_id(conn, ticket_id)) {
        Ok(Some(uuid)) => format!("ws-{}_ticket-{}", ws.workspace_uuid, uuid),
        _ => return errors::not_found_msg("Ticket not found"),
    };
    if let Err(resp) = restore_revision_snapshot(
        &app_state,
        &doc_id,
        ws.workspace_id,
        DocumentType::Ticket(ticket_id),
        &revision.yjs_document_content,
    )
    .await
    {
        return resp;
    }

    info!(ticket_id, revision_number, "Restored ticket to revision");
    HttpResponse::Ok().json(json!({
        "success": true,
        "message": format!("Restored to revision {revision_number}"),
    }))
}

// ============= Documentation Revision History API Endpoints =============

/// GET /docs/:id/revisions - List all revisions for a documentation page
pub async fn get_doc_revisions(
    doc_id: web::Path<i32>,
    mut tc: TenantConn,
    auth: AuthContext,
) -> HttpResponse {
    let doc_id = doc_id.into_inner();
    if let Err(resp) = gate_doc_page(&mut tc, &auth, doc_id) {
        return resp;
    }

    // Get all revisions
    match tc.run(|conn| crate::repository::documentation::get_documentation_revisions(conn, doc_id))
    {
        Ok(revisions) => HttpResponse::Ok().json(revisions),
        Err(_) => errors::internal("Error retrieving revisions"),
    }
}

/// GET /docs/:id/revisions/:revision_number - Get a specific revision
pub async fn get_doc_revision(
    path: web::Path<(i32, i32)>,
    mut tc: TenantConn,
    auth: AuthContext,
) -> HttpResponse {
    let (doc_id, revision_number) = path.into_inner();
    if let Err(resp) = gate_doc_page(&mut tc, &auth, doc_id) {
        return resp;
    }

    // Get the specific revision
    match tc.run(|conn| {
        crate::repository::documentation::get_documentation_revision(conn, doc_id, revision_number)
    }) {
        Ok(revision) => {
            // Encode the Yjs document snapshot as base64 for frontend
            let content_base64 = general_purpose::STANDARD.encode(&revision.yjs_document_snapshot);

            HttpResponse::Ok().json(serde_json::json!({
                "id": revision.id,
                "page_id": revision.page_id,
                "revision_number": revision.revision_number,
                "title": revision.title,
                "yjs_document_content": content_base64,
                "created_by": revision.created_by,
                "created_at": revision.created_at,
                "change_summary": revision.change_summary,
            }))
        }
        Err(_) => errors::not_found_msg("Revision not found"),
    }
}

/// POST /docs/:id/restore/:revision_number - Restore documentation page to a specific revision
pub async fn restore_doc_revision(
    path: web::Path<(i32, i32)>,
    mut tc: TenantConn,
    ws: crate::extractors::WorkspaceContext,
    app_state: web::Data<YjsAppState>,
    auth: AuthContext,
) -> HttpResponse {
    let (doc_id, revision_number) = path.into_inner();
    if let Err(resp) = gate_doc_page(&mut tc, &auth, doc_id) {
        return resp;
    }

    // Get the revision to restore
    let revision = match tc.run(|conn| {
        crate::repository::documentation::get_documentation_revision(conn, doc_id, revision_number)
    }) {
        Ok(rev) => rev,
        Err(_) => return errors::not_found_msg("Revision not found"),
    };

    // Build the same workspace-namespaced, UUID-keyed doc_id the clients
    // connect with, so the restore targets the live room.
    let doc_id_str =
        match tc.run(|conn| crate::repository::documentation::page_uuid_by_id(conn, doc_id)) {
            Ok(Some(uuid)) => format!("ws-{}_doc-{}", ws.workspace_uuid, uuid),
            _ => return errors::not_found_msg("Page not found"),
        };
    if let Err(resp) = restore_revision_snapshot(
        &app_state,
        &doc_id_str,
        ws.workspace_id,
        DocumentType::Documentation(doc_id),
        &revision.yjs_document_snapshot,
    )
    .await
    {
        return resp;
    }

    info!(
        doc_id,
        revision_number, "Restored documentation page to revision"
    );
    HttpResponse::Ok().json(json!({
        "success": true,
        "message": format!("Restored to revision {revision_number}"),
    }))
}

/// Collab handshake: resolve the WebSocket URL a client should connect
/// to for `doc_id` (Phase 2 affinity, direct-address mode). In single /
/// fly-replay mode it returns the relative WS path on this host; in
/// direct-address mode it returns the owning machine's absolute URL,
/// claiming the doc for the contacted machine if it is currently
/// unowned. Behind `dual_auth` + `WorkspaceContext`, with the same
/// namespaced-doc_id + workspace guard the WS upgrade applies.
pub async fn handshake(
    path: web::Path<String>,
    ws: crate::extractors::WorkspaceContext,
    app_state: web::Data<YjsAppState>,
) -> HttpResponse {
    let doc_id = path.into_inner();

    let parsed = match DocumentType::from_namespaced_doc_id(&doc_id) {
        Ok(p) => p,
        Err(e) => {
            warn!(doc_id = %doc_id, error = ?e, "Handshake doc_id parse failed");
            return errors::bad_request(
                "doc_id must be in the workspace-namespaced format ws-{uuid}_{kind}-{id}",
            );
        }
    };
    if parsed.workspace_uuid != ws.workspace_uuid {
        warn!(doc_id = %doc_id, "Handshake doc_id workspace mismatch");
        return HttpResponse::Forbidden().json(json!({
            "error": "doc_id workspace does not match the request workspace",
        }));
    }

    match app_state.resolve_ws_url(&doc_id).await {
        Some(ws_url) => HttpResponse::Ok().json(json!({ "ws_url": ws_url })),
        None => {
            // Direct-address mode and the owner's address is unknown
            // (owner dead or not yet registered). Tell the client to retry.
            warn!(doc_id = %doc_id, "Handshake could not resolve owner address");
            HttpResponse::ServiceUnavailable().json(json!({ "error": "owner_unreachable" }))
        }
    }
}

/// Authenticated collaboration REST endpoints (article content + the
/// revision history / restore actions for tickets and docs).
///
/// These all use the `TenantConn` extractor, so they must run behind an
/// auth middleware that injects the request context — otherwise the
/// extractor fails with 401 "Authentication required" (the bug that
/// 401'd document revisions and bounced the page to the dashboard).
/// Keeping them in one configurer means `config` applies that auth in a
/// single place, so a new endpoint can't accidentally land outside it.
fn rest_routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/handshake/{doc_id}", web::get().to(handshake))
        .route("/article/{doc_id}", web::get().to(get_article_content))
        .route(
            "/tickets/{ticket_id}/revisions",
            web::get().to(get_ticket_revisions),
        )
        .route(
            "/tickets/{ticket_id}/revisions/{revision_number}",
            web::get().to(get_ticket_revision),
        )
        .route(
            "/tickets/{ticket_id}/restore/{revision_number}",
            web::post().to(restore_ticket_revision),
        )
        .route("/docs/{doc_id}/revisions", web::get().to(get_doc_revisions))
        .route(
            "/docs/{doc_id}/revisions/{revision_number}",
            web::get().to(get_doc_revision),
        )
        .route(
            "/docs/{doc_id}/restore/{revision_number}",
            web::post().to(restore_doc_revision),
        );
}

// Configure routes
pub fn config(cfg: &mut web::ServiceConfig) {
    // The WebSocket upgrade authenticates itself (it validates the JWT
    // cookie inside ws_handler), so it stays OUT of the dual-auth
    // middleware, which would otherwise intercept the upgrade request.
    //
    // It must be a plain route, NOT its own `web::scope("")`: an
    // empty-prefix scope matches every path, so a `scope("")` here would
    // greedily claim all requests, fail to match `/ws/...` for the REST
    // paths, and 404 them before the authed scope below was ever tried.
    // A specific resource registered first matches `/ws/...` and lets
    // everything else fall through to the single authed scope.
    cfg.route("/ws/{doc_id}", web::get().to(ws_handler));

    // Everything else is authenticated REST: one auth wrap on one
    // sub-scope is the single boundary that covers every route in
    // `rest_routes`.
    cfg.service(
        web::scope("")
            .wrap(actix_web::middleware::from_fn(
                crate::middleware::dual_auth_middleware,
            ))
            .configure(rest_routes),
    );
}

#[cfg(test)]
mod content_saved_emit_tests {
    use super::*;
    use crate::schema::sync_actions;
    use crate::sync::actor::ActorContext;
    use crate::sync::session::with_actor_context;
    use crate::test_helpers::setup_test_connection;
    use diesel::prelude::*;

    // The content-saved emit must be a search-only signal: a real
    // sync_actions row (so the replicator, which drains the table, sees it)
    // but with EMPTY groups (so it fans out to no SSE subscriber / activity
    // feed) and a `*.content_saved` event type (which maps to no webhook).
    #[test]
    fn emit_content_saved_is_a_silent_sync_action() {
        let mut conn = setup_test_connection();
        let actor = ActorContext::system("test:content_saved").with_workspace(1);
        with_actor_context(&mut conn, &actor, |conn| {
            emit_content_saved(
                conn,
                crate::models::SyncAggregate::Ticket,
                42,
                "ticket.content_saved",
            )?;

            let (aggregate_id, event_type, groups): (String, String, Vec<Option<String>>) =
                sync_actions::table
                    .filter(sync_actions::event_type.eq("ticket.content_saved"))
                    .order(sync_actions::sync_id.desc())
                    .select((
                        sync_actions::aggregate_id,
                        sync_actions::event_type,
                        sync_actions::groups,
                    ))
                    .first(conn)?;

            assert_eq!(aggregate_id, "42");
            assert_eq!(event_type, "ticket.content_saved");
            assert!(
                groups.is_empty(),
                "content-saved must carry no groups so it stays search-only"
            );
            Ok::<_, diesel::result::Error>(())
        })
        .expect("emit + read back");
    }
}
