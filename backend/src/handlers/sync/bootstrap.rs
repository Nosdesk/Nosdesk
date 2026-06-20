//! `GET /api/sync/bootstrap?groups=<csv>&schema=<hash>`
//!
//! Streams an NDJSON snapshot of every aggregate row the caller's
//! granted groups can see. The response opens with a `__meta__`
//! header line, follows with one `__model__`-tagged JSON object per
//! row, and closes with `__end__`. The client streams these into the
//! object pool as they arrive — large workspaces don't block the UI
//! waiting for the whole snapshot to land.
//!
//! The bootstrap streams the bounded, every-view-needs-it aggregates
//! up front: `workflow_state`, `user`, and `asset` (always), plus
//! `documentation_collection` / `documentation_page` and `project` /
//! `project_ticket` when the workspace grant is present. Documentation
//! rows are visibility-filtered per caller (they are emitted to
//! `workspace:1` but readable per page/collection grant). Tickets,
//! comments, and attachments stay lazy-loaded through `useReference`
//! so the bootstrap stays bounded even on enterprise-scale workspaces.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use bytes::Bytes;
use diesel::prelude::*;
use futures::stream::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::error;

use crate::db::Pool;
use crate::extractors::SyncContext;
use crate::middleware::RequestContext;
use crate::models::{
    Asset, Attachment, Comment, Project, ProjectTicket, Ticket, TicketAsset, User, WorkflowState,
};
use crate::schema::{
    assets, attachments, comments, linked_tickets, project_tickets, projects, ticket_assets,
    tickets, user_emails, users, workflow_states,
};
use crate::sync::actor::ActorContext;
use crate::sync::session;

#[derive(Debug, Deserialize)]
pub struct BootstrapQuery {
    /// Comma-separated group strings the client wants to subscribe
    /// to. The server returns the intersection with the caller's
    /// permitted set in the `__meta__.groups_granted` field.
    pub groups: String,
    /// Client's persisted schema hash. When the server's compiled
    /// hash (`NOSDESK_SCHEMA_HASH`) doesn't match, the response's
    /// `__meta__` line carries the new hash so the client wipes
    /// IndexedDB before consuming the snapshot.
    #[serde(default)]
    pub schema: Option<String>,
}

const SERVER_SCHEMA_HASH: &str = env!("NOSDESK_SCHEMA_HASH");

pub async fn bootstrap(
    req: HttpRequest,
    pool: web::Data<Pool>,
    query: web::Query<BootstrapQuery>,
    ctx: SyncContext,
) -> impl Responder {
    let granted = intersect_groups(&query.groups, &ctx.allowed_groups);

    // Bounded mpsc channel so a slow client back-pressures the
    // streamer instead of buffering everything in memory.
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);

    let pool_clone = pool.clone();
    let granted_clone = granted.clone();
    // Raw requested CSV: the streamer admits `ticket:<id>` groups
    // dynamically (per-ticket `can_view_ticket`, like the SSE topic
    // path) since they aren't in the static `allowed_groups` set.
    let requested_groups = query.groups.clone();
    // The streamer needs the caller's identity to visibility-filter
    // documentation rows (emitted to workspace:1 but readable per
    // page/collection grant). Clone the User into the blocking task.
    let user = ctx.user.clone();
    // Snapshot the actor (carries the workspace pin) so the
    // spawn_blocking worker can wrap its connection in
    // `with_actor_context` and satisfy the workspace-isolation RLS
    // policies on tickets / sync_actions / workflow_states etc.
    // TenantConn isn't usable here because the streaming path runs
    // off the actix request future on a blocking thread.
    let actor = req
        .extensions()
        .get::<RequestContext>()
        .map(|rc| rc.actor.clone())
        .unwrap_or_else(|| ActorContext::user(ctx.user.uuid, Some(ctx.correlation_id)));

    // Diesel is sync; do the work on a blocking thread and ferry
    // bytes back through the channel so the Actix response future
    // can stay async-friendly.
    tokio::task::spawn_blocking(move || {
        if let Err(e) = stream_bootstrap(
            &pool_clone,
            &actor,
            &user,
            &granted_clone,
            &requested_groups,
            &tx,
        ) {
            error!(error = %e, "bootstrap streaming failed");
            // Best-effort: ship an `__error__` line so the client
            // can surface a useful message instead of just seeing
            // the stream close mid-snapshot.
            let _ = tx.blocking_send(Ok(line(json!({
                "__error__": "stream_failed",
                "detail": e.to_string(),
            }))));
        }
    });

    let body =
        ReceiverStream::new(rx).map(|r| r.map_err(actix_web::error::ErrorInternalServerError));
    HttpResponse::Ok()
        .content_type("application/x-ndjson")
        .streaming(body)
}

fn stream_bootstrap(
    pool: &web::Data<Pool>,
    actor: &ActorContext,
    user: &User,
    granted: &[String],
    requested_groups: &str,
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut conn = pool.get()?;

    // Wrap the entire streaming workload in one actor/workspace-
    // scoped transaction. Bootstrap is a consistent point-in-time
    // snapshot: doing it in a single tx pins `app.workspace_id`
    // for every query the streamer runs (incl. RLS-protected reads
    // on tickets, sync_actions, workflow_states), and gives the
    // client a snapshot-isolated read view of every aggregate.
    session::with_actor_context::<(), Box<dyn std::error::Error + Send + Sync>>(
        &mut conn,
        actor,
        |c| stream_bootstrap_inner(c, user, granted, requested_groups, tx),
    )
}

fn stream_bootstrap_inner(
    conn: &mut crate::db::DbConnection,
    user: &User,
    granted_static: &[String],
    requested_groups: &str,
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Admit `ticket:<id>` groups the caller can read on top of the
    // statically-granted set. They aren't in `allowed_for_user`
    // (a user can reach many tickets; enumerating them per request
    // is wasteful), so they're authorized dynamically per-ticket via
    // `can_view_ticket` — the same gate the SSE ticket topic uses.
    // Runs inside the actor/workspace context so the visibility query
    // sees the right RLS scope.
    // Per-viewer visibility identity, resolved once and reused for ticket
    // admission, the documentation filter, and the ticket-set scoping
    // below (restricted members must not snapshot tickets they can't see).
    let viewer = crate::sync::visibility::SyncViewer::resolve(conn, user);
    let mut granted: Vec<String> = granted_static.to_vec();
    crate::sync::groups::admit_ticket_groups(conn, requested_groups, &viewer.ctx, &mut granted);
    let granted: &[String] = &granted;

    let last_sync_id: Option<i64> = crate::schema::sync_actions::table
        .select(diesel::dsl::max(crate::schema::sync_actions::sync_id))
        .first(conn)?;
    let last_sync_id = last_sync_id.unwrap_or(0);

    // Seed the client's commit-safe feed cursor. `H_b` is the commit
    // horizon at bootstrap; everything this bootstrap can see has
    // `xid8 < H_b`, and the delta feed serves `xid8 >= H_b`, so the two
    // partition with no gap (a small re-delivery overlap is harmless —
    // the client dedupes by sync_id). Cursor `xid8 = H_b - 1` so the
    // delta's `xid8 > cursor` delivers exactly `xid8 >= H_b`. See
    // `crate::sync::feed`.
    let last_xid8 = crate::sync::feed::current_horizon(conn)?.saturating_sub(1);

    // Workspace capability flags. These are simple booleans the
    // frontend uses to gate optional UI surfaces (filter chips,
    // default visible columns, summary segments). Adding a flag
    // here is the right place when the client should treat a
    // feature as "exists for this workspace" vs "available
    // everywhere" — eg. SLA chrome should hide entirely until
    // an admin sets up at least one policy. Counts (rather than
    // "any non-archived") are fine for v1: a workspace either
    // has policies or it doesn't.
    let sla_enabled: bool = {
        use diesel::dsl::count_star;
        let n: i64 = crate::schema::sla_policies::table
            .select(count_star())
            .first(conn)
            .unwrap_or(0);
        n > 0
    };

    // Header: schema hash, cursor, granted groups, and capability
    // flags. Clients read this once at the start of every
    // bootstrap and cache the values for the session.
    send(
        tx,
        json!({
            "__meta__": {
                "server_schema": SERVER_SCHEMA_HASH,
                "last_xid8": last_xid8,
                "last_sync_id": last_sync_id,
                "groups_granted": granted,
                "sla_enabled": sla_enabled,
            }
        }),
    )?;

    // Workflow states: workspace-wide config, always sent so the
    // kanban can render columns immediately even before the
    // workflow_states store loads via its own endpoint. Captured
    // into a HashMap so the ticket loader below can denormalise
    // each ticket's workflow_state inline without a per-row query.
    let states: Vec<WorkflowState> = workflow_states::table
        .order((workflow_states::category, workflow_states::position))
        .load(conn)?;
    let mut states_by_id: std::collections::HashMap<i32, WorkflowState> =
        std::collections::HashMap::with_capacity(states.len());
    for state in &states {
        send(
            tx,
            json!({
                "__model__": "workflow_state",
                "id": state.id,
                "name": state.name,
                "category": state.category.as_str(),
                "color": state.color,
                "position": state.position,
                "is_default": state.is_default,
                "archived_at": state.archived_at,
            }),
        )?;
    }
    for state in states {
        states_by_id.insert(state.id, state);
    }

    // Users: workspace-wide set, streamed once at the start of the
    // bootstrap. Mirrors workflow_states — small finite roster, every
    // ticket / comment / assignment carries a uuid the frontend needs
    // to resolve to a name + avatar, so shipping them up-front lets
    // the table render assignee / requester cells with no follow-up
    // round-trip.
    //
    // Single-workspace deployment means "all users" in practice — the
    // permission check happens upstream in
    // `sync::groups::allowed_for_user`, but every member of
    // `workspace:1` can see the user list (it's the same set the
    // mention picker / assignee picker already query without scope).
    //
    // Email lives in `user_emails` (canonical address; the
    // `users.email` column is gone); load the primary-email lookup
    // table once into a HashMap rather than joining per-row, since
    // the `User` model is `Queryable` but not `Selectable` and tuple
    // joins would force a refactor.
    let user_rows: Vec<User> = users::table.order(users::name.asc()).load(conn)?;
    let primary_email_rows: Vec<(uuid::Uuid, String)> = user_emails::table
        .filter(user_emails::is_primary.eq(true))
        .select((user_emails::user_uuid, user_emails::email))
        .load(conn)?;
    let primary_email_by_uuid: std::collections::HashMap<uuid::Uuid, String> =
        primary_email_rows.into_iter().collect();
    // Personal dashboard layout lives in `user_preferences`; batch-load
    // it so each user's own sessions warm-start + live-sync the
    // arrangement from the pool (one query, not N+1).
    let all_user_uuids: Vec<uuid::Uuid> = user_rows.iter().map(|u| u.uuid).collect();
    let dashboard_layout_by_uuid: std::collections::HashMap<uuid::Uuid, serde_json::Value> =
        crate::repository::user_preferences::get_many(conn, &all_user_uuids)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| p.dashboard_layout.map(|dl| (p.user_uuid, dl)))
            .collect();
    for user in user_rows {
        let workspace_role = crate::repository::user_helpers::workspace_role(conn, user.uuid)
            .map(|r| r.as_str().to_string());
        send(
            tx,
            json!({
                "__model__": "user",
                "uuid": user.uuid,
                "name": user.name,
                "email": primary_email_by_uuid.get(&user.uuid).cloned().unwrap_or_default(),
                "platform_role": user.platform_role,
                "workspace_role": workspace_role,
                "pronouns": user.pronouns,
                "avatar_url": user.avatar_url,
                "avatar_thumb": user.avatar_thumb,
                "dashboard_layout": dashboard_layout_by_uuid.get(&user.uuid),
            }),
        )?;
    }

    // Assets follow the same "ship every row up-front" pattern as
    // users and workflow_states: every ticket linked-asset chip,
    // device picker, and asset list view needs an id -> name
    // lookup, and the count is bounded (~hundreds in the wild),
    // not unbounded like tickets. Shape mirrors
    // `repository::assets::asset_sync_payload` so frontend pool
    // deserialisation handles bootstrap and incremental updates
    // through one path.
    let asset_rows: Vec<Asset> = assets::table.order(assets::name.asc()).load(conn)?;
    for asset in asset_rows {
        send(
            tx,
            json!({
                "__model__": "asset",
                "id": asset.id,
                "name": asset.name,
                "kind": asset.kind,
                "serial_number": asset.serial_number,
                "manufacturer": asset.manufacturer,
                "model": asset.model,
                "asset_tag": asset.asset_tag,
                "location": asset.location,
                "primary_user_uuid": asset.primary_user_uuid,
                "attributes": asset.attributes,
                "quantity": asset.quantity,
                "unit": asset.unit,
                "external_sync_source": asset.external_sync_source,
            }),
        )?;
    }

    // Two project-loading paths:
    //
    // 1. Workspace-wide (`workspace:1` in granted set): load every
    //    project the user has visibility into. Single-workspace
    //    deployment means this is "all projects" in practice; the
    //    permission check happens upstream in
    //    `sync::groups::allowed_for_user`.
    //
    // 2. Per-project (`project:<id>` strings in granted set):
    //    incremental subscribe-on-route-entry. Drop the prefix,
    //    parse the suffix as i32, fetch the matching projects.
    //
    // Both paths land in the same set; HashSet dedupes if a request
    // ever asks for both `workspace:1` and `project:7` together.
    use std::collections::HashSet;
    let want_all = granted.iter().any(|g| g == "workspace:1");

    // Documentation: workspace-wide knowledge base. Stream every
    // collection and page (all statuses, so the index / archived /
    // drafts / trash views all derive from the same pool) when the
    // caller has the workspace grant. Documentation rows are emitted
    // to `workspace:1`, so without a per-row visibility filter a
    // member would receive metadata for pages restricted away from
    // them. This mirrors the read-side filter on /api/sync/delta;
    // both reuse the canonical access logic so they cannot drift.
    if want_all {
        let is_admin = viewer.is_doc_admin;

        let collections: Vec<crate::models::DocumentationCollection> =
            crate::schema::documentation_collections::table.load(conn)?;
        for c in collections {
            if !is_admin
                && !crate::repository::documentation_collections::can_user_access_collection(
                    conn, c.id, &user.uuid, false,
                )?
            {
                continue;
            }
            send(
                tx,
                json!({
                    "__model__": "documentation_collection",
                    "id": c.id,
                    "uuid": c.uuid,
                    "name": c.name,
                    "slug": c.slug,
                    "description": c.description,
                    "icon": c.icon,
                    "color": c.color,
                    "is_system": c.is_system,
                    "created_by": c.created_by,
                    "display_order": c.display_order,
                    "description_text": c.description_text,
                    "hide_titles_from_non_members": c.hide_titles_from_non_members,
                    "created_at": c.created_at,
                    "updated_at": c.updated_at,
                }),
            )?;
        }

        let all_pages: Vec<crate::models::DocumentationPage> =
            crate::schema::documentation_pages::table.load(conn)?;
        let visible_pages = crate::repository::documentation::filter_pages_for_user(
            conn, all_pages, &user.uuid, is_admin,
        )?;
        // Denormalised collection membership (one collection per page,
        // UNIQUE(page_id)) so the page row is self-contained for the
        // pool — mirrors `page_sync_payload`'s collection_id field.
        let visible_page_ids: Vec<i32> = visible_pages.iter().map(|p| p.id).collect();
        let collection_by_page: std::collections::HashMap<i32, i32> =
            crate::schema::documentation_collection_pages::table
                .filter(
                    crate::schema::documentation_collection_pages::page_id
                        .eq_any(&visible_page_ids),
                )
                .select((
                    crate::schema::documentation_collection_pages::page_id,
                    crate::schema::documentation_collection_pages::collection_id,
                ))
                .load::<(i32, i32)>(conn)?
                .into_iter()
                .collect();
        for p in visible_pages {
            send(
                tx,
                json!({
                    "__model__": "documentation_page",
                    "id": p.id,
                    "uuid": p.uuid,
                    "collection_id": collection_by_page.get(&p.id),
                    "title": p.title,
                    "slug": p.slug,
                    "icon": p.icon,
                    "cover_image": p.cover_image,
                    "status": p.status,
                    "parent_id": p.parent_id,
                    "display_order": p.display_order,
                    "is_public": p.is_public,
                    "is_template": p.is_template,
                    "archived_at": p.archived_at,
                    "deleted_at": p.deleted_at,
                    "created_by": p.created_by,
                    "last_edited_by": p.last_edited_by,
                    "verified_by": p.verified_by,
                    "verified_at": p.verified_at,
                    "verify_interval_days": p.verify_interval_days,
                    "created_at": p.created_at,
                    "updated_at": p.updated_at,
                }),
            )?;
        }
    }

    let project_ids: Vec<i32> = if want_all {
        projects::table.select(projects::id).load::<i32>(conn)?
    } else {
        let mut ids: HashSet<i32> = HashSet::new();
        for g in granted {
            if let Some(suffix) = g.strip_prefix("project:") {
                if let Ok(id) = suffix.parse::<i32>() {
                    ids.insert(id);
                }
            }
        }
        ids.into_iter().collect()
    };

    if !project_ids.is_empty() {
        let projects: Vec<Project> = projects::table
            .filter(projects::id.eq_any(&project_ids))
            .load(conn)?;
        for p in projects {
            send(
                tx,
                json!({
                    "__model__": "project",
                    "id": p.id,
                    "name": p.name,
                    "description": p.description,
                    "status": p.status,
                    "created_at": p.created_at,
                    "updated_at": p.updated_at,
                    "created_by": p.created_by,
                }),
            )?;
        }

        let assocs: Vec<ProjectTicket> = project_tickets::table
            .filter(project_tickets::project_id.eq_any(&project_ids))
            .load(conn)?;
        for a in assocs {
            send(
                tx,
                json!({
                    "__model__": "project_ticket",
                    "project_id": a.project_id,
                    "ticket_id": a.ticket_id,
                    "display_order": a.display_order,
                }),
            )?;
        }
    }

    // Tickets: two paths, mirroring the project loader above.
    //
    // 1. Workspace-wide (`workspace:1` granted): every ticket in the
    //    workspace. The TicketsListViewV2 reads from this — My Queue
    //    and Triage filter on assignee / workflow_state.category
    //    respectively, both of which need the full ticket set.
    //
    // 2. Per-project: tickets associated with the granted project ids
    //    via project_tickets. The kanban view reads from this.
    //
    // The two paths produce the same denormalised ticket shape; we
    // load whichever set the granted groups call for, deduping
    // implicitly through eq_any-on-id.
    // Tickets explicitly subscribed by id (the pool-native detail
    // view's `ticket:<id>` group). Streamed with the same denormalised
    // payload as the list/board sets, plus their related rows below.
    let detail_ticket_ids: Vec<i32> = granted
        .iter()
        .filter_map(|g| {
            g.strip_prefix("ticket:")
                .and_then(|s| s.parse::<i32>().ok())
        })
        .collect();

    let ticket_query = if want_all {
        // Restricted members see only their requester/watcher set; staff
        // see every ticket (the query all-passes for sees_all).
        crate::sync::visibility::bootstrap_ticket_query(&viewer)
    } else {
        // Project-scoped tickets (kanban) unioned with any tickets
        // subscribed by id (detail view). Empty in both means the
        // caller asked for neither a workspace/project nor a ticket
        // group, so there's nothing to stream.
        let mut scoped_ids: Vec<i32> = if project_ids.is_empty() {
            Vec::new()
        } else {
            project_tickets::table
                .filter(project_tickets::project_id.eq_any(&project_ids))
                .select(project_tickets::ticket_id)
                .load(conn)?
        };
        scoped_ids.extend(detail_ticket_ids.iter().copied());
        if scoped_ids.is_empty() {
            return finish(tx, last_xid8, last_sync_id);
        }
        // Restrict to tickets this viewer can actually read (staff
        // all-pass; a member with a project grant still only sees their
        // own tickets within it). detail_ticket_ids are already
        // can_view-gated via admit_ticket_groups, so they survive.
        let visible: Vec<i32> = crate::repository::ticket_visibility::visible_ticket_ids(
            conn,
            &viewer.ctx,
            &scoped_ids,
        )?
        .into_iter()
        .collect();
        if visible.is_empty() {
            return finish(tx, last_xid8, last_sync_id);
        }
        tickets::table
            .filter(tickets::id.eq_any(visible))
            .into_boxed()
    };

    let ticket_rows: Vec<Ticket> = ticket_query.load(conn)?;

    // Per-ticket pill data computed in one batch each so the
    // bootstrap stays O(n) rather than N round-trips. Empty maps
    // for the tickets without signals / devices; consumers default
    // those to 'none' / null.
    let ticket_ids: Vec<i32> = ticket_rows.iter().map(|t| t.id).collect();
    let kb_gap_counts =
        crate::repository::knowledge_gaps::open_signal_counts_for_tickets(conn, &ticket_ids)?;
    let device_summaries =
        crate::repository::tickets::devices_summary_for_tickets(conn, &ticket_ids)?;
    let cycle_membership = crate::repository::cycles::cycle_ids_for_tickets(conn, &ticket_ids)?;
    // Tag id list per ticket. Same batched-lookup pattern the
    // cycle membership uses; empty Vec when a ticket has no tags.
    let tag_membership = crate::repository::tags::tag_ids_for_tickets(conn, &ticket_ids)?;
    let merge_membership = crate::repository::ticket_merge::merges_for_tickets(conn, &ticket_ids)?;
    let watcher_membership =
        crate::repository::ticket_watchers::watcher_uuids_for_tickets(conn, &ticket_ids)?;
    // Load every SLA policy + working calendar once; the
    // pill-computation loop below resolves each ticket against
    // them in memory.
    let sla_ctx = crate::repository::sla::load_for_pill_computation(conn)?;
    // Batch-load the group memberships for every distinct assignee
    // in this ticket set so the matcher can honour
    // `assignee_group_id_filter` without an N+1.
    let assignee_uuids: Vec<uuid::Uuid> =
        ticket_rows.iter().filter_map(|t| t.assignee_uuid).collect();
    let groups_by_assignee =
        crate::repository::groups::get_group_ids_for_users(conn, &assignee_uuids)
            .unwrap_or_default();
    let now = chrono::Utc::now();

    for t in ticket_rows {
        let ws = states_by_id.get(&t.workflow_state_id);
        // Same nested shape the create/update emits build, via the one
        // shared helper, so the card's state can't drift across paths.
        let workflow_state_payload = ws.map(|s| crate::repository::tickets::workflow_state_json(s));
        let kb_gap_signal = match kb_gap_counts.get(&t.id).copied().unwrap_or(0) {
            0 => "none",
            1..=2 => "weak",
            _ => "strong",
        };
        let affected_devices = device_summaries.get(&t.id).map(|(count, id, name, os)| {
            json!({
                "count": count,
                "first": { "id": id, "name": name, "os": os },
            })
        });
        // SLA pill: pick the most-specific applicable policy, then
        // resolve the working calendar + holidays it points at.
        // Tickets without a matching policy or calendar render
        // without a pill; consumers tolerate the null shape.
        let assignee_groups = t
            .assignee_uuid
            .and_then(|u| groups_by_assignee.get(&u))
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let sla = crate::services::sla::pick_policy(&sla_ctx.policies, &t, assignee_groups)
            .and_then(|policy| {
                let cal_id = policy.working_calendar_id?;
                let calendar = sla_ctx.calendars_by_id.get(&cal_id)?;
                let holidays = sla_ctx
                    .holidays_by_calendar
                    .get(&cal_id)
                    .cloned()
                    .unwrap_or_default();
                // Missing state row (shouldn't happen but possible if
                // a state was hard-deleted) defaults to paused so we
                // don't silently start counting on an unresolvable
                // category.
                let paused = ws.map(|s| s.pauses_sla).unwrap_or(true);
                crate::services::sla::compute_pill(&t, paused, policy, calendar, &holidays, now)
            })
            .and_then(|pill| serde_json::to_value(pill).ok())
            .unwrap_or(serde_json::Value::Null);
        send(
            tx,
            json!({
                "__model__": "ticket",
                "id": t.id,
                "uuid": t.uuid,
                "title": t.title,
                "workflow_state": workflow_state_payload,
                "workflow_state_id": t.workflow_state_id,
                "priority": match t.priority {
                    crate::models::TicketPriority::None => "none",
                    crate::models::TicketPriority::Low => "low",
                    crate::models::TicketPriority::Medium => "medium",
                    crate::models::TicketPriority::High => "high",
                    crate::models::TicketPriority::Urgent => "urgent",
                },
                "requester_uuid": t.requester_uuid,
                "assignee_uuid": t.assignee_uuid,
                "category_id": t.category_id,
                "triage_state": t.triage_state,
                "due_date": t.due_date,
                "resolution_notes": t.resolution_notes,
                // Detail-view scalars (source row + audit bylines) so
                // the pool-native ticket detail view renders them with
                // no REST fetch. Immutable / rarely-changing, so list
                // views carry them harmlessly and ignore them.
                "created_by": t.created_by,
                "closed_by": t.closed_by,
                "closed_at": t.closed_at,
                "submitted_via": t.submitted_via,
                "origin_channel_id": t.origin_channel_id,
                // Merge state so the pool-native ticket detail view can
                // render the merged-into banner + read-only composer
                // without a REST fetch (Phase 2).
                "merged_into_ticket_id": merge_membership.get(&t.id).map(|m| m.merged_into_ticket_id),
                "merged_at": merge_membership.get(&t.id).map(|m| m.merged_at),
                "merged_by_user_uuid": merge_membership.get(&t.id).and_then(|m| m.merged_by_user_uuid),
                "kb_gap_signal": kb_gap_signal,
                "affected_devices": affected_devices,
                "cycle_id": cycle_membership.get(&t.id),
                "sla": sla,
                "recurrence_rule": t.recurrence_rule,
                "tag_ids": tag_membership.get(&t.id).cloned().unwrap_or_default(),
                "watcher_uuids": watcher_membership.get(&t.id).cloned().unwrap_or_default(),
                "recurrence_template_id": t.recurrence_template_id,
                "created_at": t.created_at,
                "updated_at": t.updated_at,
                "last_activity_at": t.updated_at,
            }),
        )?;
    }

    // Pool-native ticket detail: stream the open ticket's related
    // rows (comments, attachments, device + ticket links, project
    // memberships). The global / project bootstraps deliberately omit
    // these, so they only ship for an explicit `ticket:<id>` group.
    if !detail_ticket_ids.is_empty() {
        stream_ticket_detail_extras(conn, &detail_ticket_ids, viewer.ctx.sees_all(), tx)?;
    }

    finish(tx, last_xid8, last_sync_id)
}

/// Stream the related rows the pool-native ticket detail view reads
/// for the given tickets: comments, their attachments, device links
/// (`ticket_asset`), ticket links (`linked_ticket`, directional rows
/// where the ticket is the subject), and project memberships plus the
/// referenced project rows (for the project chips). Referenced users /
/// assets / cycles aren't streamed here — the user + asset rosters
/// already shipped earlier in the bootstrap, and the cycle chip
/// resolves via the lazy `cycle` reference fetcher.
fn stream_ticket_detail_extras(
    conn: &mut crate::db::DbConnection,
    ticket_ids: &[i32],
    sees_all: bool,
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Restricted viewers see only public comments on their visible
    // tickets (mirrors `get_public_comments_by_ticket_id`); their
    // attachments then scope to the surviving comment ids automatically.
    let mut comment_query = comments::table
        .filter(comments::ticket_id.eq_any(ticket_ids))
        .into_boxed();
    if !sees_all {
        comment_query = comment_query.filter(comments::is_internal.eq(false));
    }
    let comment_rows: Vec<Comment> = comment_query.order(comments::created_at.asc()).load(conn)?;
    let comment_ids: Vec<i32> = comment_rows.iter().map(|c| c.id).collect();
    for c in &comment_rows {
        // Render essentials only (mirrors the `comment.created` emit):
        // heavy email-only fields stay a lazy REST fetch on expand.
        send(
            tx,
            json!({
                "__model__": "comment",
                "id": c.id,
                "ticket_id": c.ticket_id,
                "user_uuid": c.user_uuid,
                "content": c.content,
                "is_internal": c.is_internal,
                "content_format": c.content_format,
                // Mirrors the `comment.created` emit: the render tier
                // must travel with the bootstrap so the pool-native view
                // picks inline vs iframe correctly without a REST fetch.
                "render_kind": c.render_kind,
                "created_at": c.created_at,
            }),
        )?;
    }

    if !comment_ids.is_empty() {
        let attachment_rows: Vec<Attachment> = attachments::table
            .filter(attachments::comment_id.eq_any(&comment_ids))
            .load(conn)?;
        for a in attachment_rows {
            send(
                tx,
                json!({
                    "__model__": "attachment",
                    "id": a.id,
                    "comment_id": a.comment_id,
                    "name": a.name,
                    "url": a.url,
                    "mime_type": a.mime_type,
                    "file_size": a.file_size,
                }),
            )?;
        }
    }

    let asset_links: Vec<TicketAsset> = ticket_assets::table
        .filter(ticket_assets::ticket_id.eq_any(ticket_ids))
        .load(conn)?;
    for ta in asset_links {
        send(
            tx,
            json!({
                "__model__": "ticket_asset",
                "ticket_id": ta.ticket_id,
                "asset_id": ta.asset_id,
            }),
        )?;
    }

    // Directional rows where the subscribed ticket is the subject —
    // matches the two-directional rows `link_tickets` writes, so the
    // client filter (`ticket_id === id`) lands every link.
    let links: Vec<(i32, i32)> = linked_tickets::table
        .filter(linked_tickets::ticket_id.eq_any(ticket_ids))
        .select((linked_tickets::ticket_id, linked_tickets::linked_ticket_id))
        .load(conn)?;
    for (tid, lid) in links {
        send(
            tx,
            json!({
                "__model__": "linked_ticket",
                "ticket_id": tid,
                "linked_ticket_id": lid,
            }),
        )?;
    }

    let memberships: Vec<ProjectTicket> = project_tickets::table
        .filter(project_tickets::ticket_id.eq_any(ticket_ids))
        .load(conn)?;
    let mut member_project_ids: Vec<i32> = memberships.iter().map(|m| m.project_id).collect();
    member_project_ids.sort_unstable();
    member_project_ids.dedup();
    for m in &memberships {
        send(
            tx,
            json!({
                "__model__": "project_ticket",
                "project_id": m.project_id,
                "ticket_id": m.ticket_id,
                "display_order": m.display_order,
            }),
        )?;
    }
    if !member_project_ids.is_empty() {
        let project_rows: Vec<Project> = projects::table
            .filter(projects::id.eq_any(&member_project_ids))
            .load(conn)?;
        for p in project_rows {
            send(
                tx,
                json!({
                    "__model__": "project",
                    "id": p.id,
                    "name": p.name,
                    "description": p.description,
                    "status": p.status,
                    "created_at": p.created_at,
                    "updated_at": p.updated_at,
                    "created_by": p.created_by,
                }),
            )?;
        }
    }

    Ok(())
}

fn finish(
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
    last_xid8: i64,
    last_sync_id: i64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    send(
        tx,
        json!({ "__end__": { "last_xid8": last_xid8, "last_sync_id": last_sync_id } }),
    )?;
    Ok(())
}

fn send(
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
    value: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if tx.blocking_send(Ok(line(value))).is_err() {
        // Receiver dropped — client disconnected. Bail out of the
        // loop without surfacing as an error; the spawn_blocking
        // task ends, the connection releases, no rows leaked.
        return Err("client disconnected".into());
    }
    Ok(())
}

fn line(value: serde_json::Value) -> Bytes {
    let mut s = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    s.push('\n');
    Bytes::from(s)
}

fn intersect_groups(requested_csv: &str, allowed: &[String]) -> Vec<String> {
    use std::collections::HashSet;
    let allowed_set: HashSet<&str> = allowed.iter().map(|s| s.as_str()).collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for raw in requested_csv.split(',') {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !allowed_set.contains(trimmed) {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

// `intersect_groups` is duplicated between bootstrap.rs and delta.rs
// intentionally for now — the helper is small, the call site
// constraints are subtly different (bootstrap echoes the granted set
// in the response while delta short-circuits on empty), and pulling
// to a shared module would force both sites to take a Vec<String>
// allocation for what's already a tiny stack-allocated structure.
// Revisit if the helper grows past 30 lines or a third caller appears.

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn schema_hash_is_compile_time_stamped() {
        // Sanity check that build.rs ran — empty schema hash would
        // mean the bootstrap response advertises no schema, and
        // every client would treat their cached state as out of
        // sync on every cold start.
        assert!(!SERVER_SCHEMA_HASH.is_empty());
    }
}
