//! `seed_demo` — fill a dev instance's bootstrap workspace with a
//! realistic IT-helpdesk demo dataset (users, projects/cycles, devices,
//! tickets with rich-text bodies and comment threads). Invoked by
//! `make seed-demo`.
//!
//! Design notes:
//! - Reuses the same sync-wired repository paths the app uses, so every
//!   seeded row emits the same `sync_actions` events real writes do.
//! - Runs entirely as `nosdesk_app` inside one bootstrap actor context
//!   (`app.workspace_id = 1`), exactly like `import_tickets`. No
//!   privileged role is needed.
//! - Timestamps in the dataset are relative offsets; a backdating pass
//!   rewrites `created_at`/`closed_at`/comment times (and the handful of
//!   `cycle_ticket.added` events that drive burnup) directly after
//!   insert, so dashboards and burnup charts look alive rather than
//!   spawned all at once. Direct UPDATEs in a binary are exempt from the
//!   repository sync-emit lint and emit no extra activity.
//! - The whole run is one transaction: it either seeds everything or
//!   nothing.

extern crate backend;

use backend::db;
use backend::db::DbConnection;
use backend::models::{
    ContentFormat, Cycle, NewArticleContent, NewAsset, NewComment, NewCycle, NewProject, NewTicket,
    NewUserEmail, PlatformRole, ProjectStatus, SyncAggregate, SyncOp, TicketPriority,
    WorkflowStateCategory,
};
use backend::repository;
use backend::schema::{
    comments, cycle_tickets, cycles, sync_actions, tickets, user_emails, users, workspaces,
};
use backend::services::seed::markdown_to_yjs;
use backend::sync::actor::{ActorContext, BOOTSTRAP_WORKSPACE_ID};
use backend::sync::emit::{self, SyncEmit};
use backend::sync::groups;
use backend::sync::session::with_actor_context;
use backend::utils::NewUserBuilder;

use chrono::{DateTime, Datelike, Duration, Utc};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Timestamptz};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::process::ExitCode;
use uuid::Uuid;

const DEMO_EMAIL_DOMAIN: &str = "@demo.nosdesk.test";

// ---------------------------------------------------------------------------
// Dataset shape (backend/seeds/demo.json). Unknown JSON keys (e.g. the
// leading `_comment`, per-user `title`) are ignored by serde.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct DemoData {
    password: String,
    users: Vec<DemoUser>,
    projects: Vec<DemoProject>,
    devices: Vec<DemoDevice>,
    tickets: Vec<DemoTicket>,
}

#[derive(Deserialize)]
struct DemoUser {
    key: String,
    name: String,
    email: String,
    /// "agent" or "member" — the workspace membership role.
    role: String,
}

#[derive(Deserialize)]
struct DemoProject {
    key: String,
    name: String,
    description: Option<String>,
    cycles: Vec<DemoCycle>,
}

#[derive(Deserialize)]
struct DemoCycle {
    key: String,
    name: String,
    /// "active" or "completed".
    state: String,
    start_days_ago: i64,
    /// Negative means the cycle ends in the future (an active cycle).
    end_days_ago: i64,
}

#[derive(Deserialize)]
struct DemoDevice {
    key: String,
    name: String,
    manufacturer: Option<String>,
    model: Option<String>,
    serial_number: Option<String>,
    asset_tag: Option<String>,
    location: Option<String>,
    #[serde(default)]
    primary_user: Option<String>,
    #[serde(default)]
    attributes: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct DemoTicket {
    title: String,
    body_md: String,
    category: String,
    priority: String,
    state_category: String,
    requester: String,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    cycle: Option<String>,
    #[serde(default)]
    device: Option<String>,
    created_days_ago: i64,
    #[serde(default)]
    closed_hours_after: Option<i64>,
    #[serde(default)]
    comments: Vec<DemoComment>,
}

#[derive(Deserialize)]
struct DemoComment {
    author: String,
    body: String,
    minutes_after: i64,
    #[serde(default)]
    internal: bool,
}

/// Low-detail ticket templates (backend/seeds/demo-history.json) that the
/// history phase expands across the last 365 days to populate the activity
/// heatmap and long-range analytics. No bodies/comments/links.
#[derive(Deserialize)]
struct DemoHistory {
    tickets: Vec<HistoryTemplate>,
}

#[derive(Deserialize)]
struct HistoryTemplate {
    title: String,
    category: String,
    priority: String,
}

// ---------------------------------------------------------------------------
// Errors: a thin wrapper so `?` on diesel calls composes with our own
// precondition/validation failures inside the actor transaction.
// ---------------------------------------------------------------------------

enum SeedError {
    /// A precondition failed (no workspace, no admin). Fatal, exit 1.
    Precondition(String),
    /// Demo data already present. Not an error to the operator; exit 0.
    AlreadySeeded,
    /// A reference in the dataset didn't resolve, etc. Fatal, exit 1.
    Data(String),
    Db(diesel::result::Error),
}

impl From<diesel::result::Error> for SeedError {
    fn from(e: diesel::result::Error) -> Self {
        SeedError::Db(e)
    }
}

#[derive(Default)]
struct Summary {
    users: usize,
    projects: usize,
    cycles: usize,
    devices: usize,
    tickets: usize,
    comments: usize,
    bodies_skipped: usize,
    history: usize,
}

fn main() -> ExitCode {
    let raw = include_str!("../../seeds/demo.json");
    let data: DemoData = match serde_json::from_str(raw) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("seed_demo: failed to parse embedded demo.json: {e}");
            return ExitCode::FAILURE;
        }
    };

    let history: DemoHistory =
        match serde_json::from_str(include_str!("../../seeds/demo-history.json")) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("seed_demo: failed to parse embedded demo-history.json: {e}");
                return ExitCode::FAILURE;
            }
        };

    // Hash the shared demo password once, outside the DB transaction so
    // the bcrypt error type never has to compose with diesel's.
    let password_hash = match bcrypt::hash(&data.password, bcrypt::DEFAULT_COST) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("seed_demo: bcrypt hash failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let pool = db::establish_connection_pool();
    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("seed_demo: database connection error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let actor = ActorContext::bootstrap("cli:seed_demo");
    let result: Result<Summary, SeedError> = with_actor_context(&mut conn, &actor, |c| {
        seed(c, &data, &history, &password_hash)
    });

    match result {
        Ok(summary) => {
            print_summary(&summary, &data);
            ExitCode::SUCCESS
        }
        Err(SeedError::AlreadySeeded) => {
            println!("Demo data already present (found {DEMO_EMAIL_DOMAIN} users). Nothing to do.");
            println!(
                "Reset with `make clean-db`, complete onboarding, then re-run `make seed-demo`."
            );
            ExitCode::SUCCESS
        }
        Err(SeedError::Precondition(msg)) => {
            eprintln!("seed_demo: {msg}");
            ExitCode::FAILURE
        }
        Err(SeedError::Data(msg)) => {
            eprintln!("seed_demo: dataset error: {msg}");
            ExitCode::FAILURE
        }
        Err(SeedError::Db(e)) => {
            eprintln!("seed_demo: database error (nothing was written): {e}");
            ExitCode::FAILURE
        }
    }
}

fn seed(
    conn: &mut DbConnection,
    data: &DemoData,
    history: &DemoHistory,
    password_hash: &str,
) -> Result<Summary, SeedError> {
    let now = Utc::now();
    let mut summary = Summary::default();

    let admin_uuid = preflight(conn)?;

    // Idempotent belt-and-braces: guarantees workflow states / categories
    // exist even on an unusual instance where onboarding ran a partial path.
    backend::services::seed::seed_workspace_defaults(conn, Some(admin_uuid))?;

    // --- Users -------------------------------------------------------------
    // key -> (uuid, is_agent)
    let mut user_map: HashMap<String, (Uuid, bool)> = HashMap::new();
    for du in &data.users {
        let is_agent = du.role == "agent";
        let membership_role = if is_agent { "agent" } else { "member" };

        let new_user =
            NewUserBuilder::local_user(du.name.clone(), du.email.clone(), PlatformRole::User)
                .build();
        let user = repository::create_user(new_user, conn)?;

        repository::workspaces::add_membership(
            conn,
            BOOTSTRAP_WORKSPACE_ID,
            user.uuid,
            membership_role,
        )?;

        diesel::insert_into(user_emails::table)
            .values(&NewUserEmail {
                user_uuid: user.uuid,
                email: du.email.clone(),
                email_type: "personal".to_string(),
                is_primary: true,
                is_verified: true,
                source: Some("demo-seed".to_string()),
            })
            .execute(conn)?;

        insert_local_identity(conn, user.uuid, &du.email, password_hash)?;

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::User,
                aggregate_id: user.uuid.to_string(),
                op: SyncOp::Insert,
                event_type: "user.created",
                data: json!({
                    "uuid": user.uuid,
                    "name": user.name,
                    "email": du.email,
                    "platform_role": "user",
                    "workspace_role": membership_role,
                    "avatar_url": null,
                    "avatar_thumb": null,
                }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;

        user_map.insert(du.key.clone(), (user.uuid, is_agent));
        summary.users += 1;
    }

    let resolve_user = |key: &str| -> Result<(Uuid, bool), SeedError> {
        user_map
            .get(key)
            .copied()
            .ok_or_else(|| SeedError::Data(format!("unknown user key `{key}`")))
    };

    // --- Projects & cycles -------------------------------------------------
    let mut project_map: HashMap<String, i32> = HashMap::new();
    // cycle key -> cycle_id
    let mut cycle_map: HashMap<String, i32> = HashMap::new();
    // Cycles to finalise (state "completed") once their tickets are attached,
    // paired with their target completed-at timestamp.
    let mut completed_cycles: Vec<(Cycle, DateTime<Utc>)> = Vec::new();
    for dp in &data.projects {
        let project = repository::projects::create_project(
            conn,
            NewProject {
                name: dp.name.clone(),
                description: dp.description.clone(),
                status: ProjectStatus::Active,
                start_date: None,
                end_date: None,
            },
            None,
        )?;
        project_map.insert(dp.key.clone(), project.id);
        summary.projects += 1;

        for dc in &dp.cycles {
            let want_completed = dc.state == "completed";
            // Park completed cycles in `planned` at creation: it keeps
            // completed_at NULL (the cycles_completed_snapshot check) and out
            // of the one-active-per-project unique index while the sibling
            // active cycle is created. They're finalised via cycles::complete
            // after their tickets attach, so the snapshot reflects membership.
            let create_state = if want_completed {
                "planned"
            } else {
                dc.state.as_str()
            };
            let end_at = now - Duration::days(dc.end_days_ago);
            let cycle = repository::cycles::create(
                conn,
                NewCycle {
                    project_id: project.id,
                    name: dc.name.clone(),
                    start_at: Some(now - Duration::days(dc.start_days_ago)),
                    end_at: Some(end_at),
                    state: create_state.to_string(),
                    created_by: Some(admin_uuid),
                },
            )?;
            cycle_map.insert(dc.key.clone(), cycle.id);
            if want_completed {
                completed_cycles.push((cycle, end_at));
            }
            summary.cycles += 1;
        }
    }

    // --- Devices -----------------------------------------------------------
    let mut device_map: HashMap<String, i32> = HashMap::new();
    for dd in &data.devices {
        let primary_user_uuid = match &dd.primary_user {
            Some(k) => Some(resolve_user(k)?.0),
            None => None,
        };
        let asset = repository::assets::create_device(
            conn,
            NewAsset {
                name: dd.name.clone(),
                serial_number: dd.serial_number.clone(),
                manufacturer: dd.manufacturer.clone(),
                model: dd.model.clone(),
                location: dd.location.clone(),
                notes: None,
                primary_user_uuid,
                purchase_date: None,
                asset_tag: dd.asset_tag.clone(),
                kind: "device".to_string(),
                attributes: dd.attributes.clone().unwrap_or_else(|| json!({})),
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
            },
        )?;
        device_map.insert(dd.key.clone(), asset.id);
        summary.devices += 1;
    }

    let category_ids: HashMap<String, i32> = repository::categories::get_all_categories(conn)?
        .into_iter()
        .map(|c| (c.name, c.id))
        .collect();

    // --- Tickets -----------------------------------------------------------
    for dt in &data.tickets {
        let category = parse_state_category(&dt.state_category);
        let workflow_state = repository::workflow_states::first_in_category(conn, category)?;
        let category_id = category_ids.get(&dt.category).copied();
        let (requester_uuid, _) = resolve_user(&dt.requester)?;
        let assignee_uuid = match &dt.assignee {
            Some(k) => Some(resolve_user(k)?.0),
            None => None,
        };

        let new_ticket = NewTicket {
            title: dt.title.clone(),
            workflow_state_id: workflow_state.id,
            priority: parse_priority(&dt.priority),
            requester_uuid: Some(requester_uuid),
            assignee_uuid,
            category_id,
            submitted_via: Some("web".to_string()),
            guest_lookup_token: None,
            verification_state: None,
            origin_channel_id: None,
            triage_state: None,
            due_date: None,
            start_date: None,
            recurrence_rule: None,
            recurrence_template_id: None,
            resolution_notes: None,
            spam_suspected: false,
        };

        let project_id = match &dt.project {
            Some(k) => Some(
                *project_map
                    .get(k)
                    .ok_or_else(|| SeedError::Data(format!("unknown project key `{k}`")))?,
            ),
            None => None,
        };

        let ticket = match project_id {
            Some(pid) => repository::tickets::create_ticket_in_project(conn, new_ticket, pid)?,
            None => repository::tickets::create_ticket(conn, new_ticket)?,
        };
        summary.tickets += 1;

        // Body: markdown -> Yjs, via the same converter the welcome page uses.
        match markdown_to_yjs(&dt.body_md) {
            Some(yjs_document) => {
                repository::article_content::create_article_content(
                    conn,
                    NewArticleContent {
                        ticket_id: ticket.id,
                        yjs_state_vector: None,
                        yjs_document: Some(yjs_document),
                        yjs_client_id: None,
                    },
                )?;
            }
            None => summary.bodies_skipped += 1,
        }

        let created_at = now - Duration::days(dt.created_days_ago);
        let mut updated_at = created_at;
        let mut first_response_at: Option<DateTime<Utc>> = None;

        // Comments.
        for dc in &dt.comments {
            let (author_uuid, author_is_agent) = resolve_user(&dc.author)?;
            let comment = repository::comments::create_comment(
                conn,
                NewComment {
                    content: dc.body.clone(),
                    ticket_id: ticket.id,
                    user_uuid: author_uuid,
                    channel_metadata: None,
                    is_internal: dc.internal,
                    // Demo bodies are plain prose; declaring them html
                    // (the ContentFormat default) routed them into the
                    // frontend's legacy email-iframe fallback.
                    content_format: ContentFormat::Plaintext,
                    body_text: None,
                    body_html: None,
                    new_content: None,
                    quoted_content: None,
                    raw_source_uri: None,
                    render_kind: None,
                },
                None,
            )?;
            summary.comments += 1;

            let comment_at = created_at + Duration::minutes(dc.minutes_after);
            if comment_at > updated_at {
                updated_at = comment_at;
            }
            // First agent reply drives the SLA first-response clock so
            // backdated open tickets don't all render as breached.
            if author_is_agent && !dc.internal && first_response_at.is_none() {
                first_response_at = Some(comment_at);
            }

            backdate_comment(conn, comment.id, comment_at)?;
        }

        // Device link.
        if let Some(k) = &dt.device {
            let asset_id = *device_map
                .get(k)
                .ok_or_else(|| SeedError::Data(format!("unknown device key `{k}`")))?;
            repository::tickets::add_device_to_ticket(conn, ticket.id, asset_id)?;
        }

        // Cycle membership + burnup backdating.
        if let Some(k) = &dt.cycle {
            let cycle_id = *cycle_map
                .get(k)
                .ok_or_else(|| SeedError::Data(format!("unknown cycle key `{k}`")))?;
            let cycle_actor = assignee_uuid.unwrap_or(admin_uuid);
            repository::cycles::add_ticket(conn, cycle_id, ticket.id, Some(cycle_actor))?;
            backdate_cycle_membership(conn, cycle_id, ticket.id, created_at)?;
        }

        // Closed timestamp for terminal states.
        let closed_at = match category {
            WorkflowStateCategory::Done | WorkflowStateCategory::Cancelled => {
                dt.closed_hours_after.map(|h| {
                    let c = created_at + Duration::hours(h);
                    if c > updated_at {
                        updated_at = c;
                    }
                    c
                })
            }
            _ => None,
        };

        backdate_ticket(
            conn,
            ticket.id,
            created_at,
            updated_at,
            closed_at,
            first_response_at,
        )?;
    }

    // Finalise completed cycles now that their tickets are attached, so the
    // completion snapshot reflects real membership. complete() stamps
    // completed_at = now(); backdate it to the cycle's end for a realistic past
    // cycle.
    for (cycle, end_at) in &completed_cycles {
        let snapshot = repository::cycles::build_completion_snapshot(conn, cycle)?;
        repository::cycles::complete(conn, cycle.uuid, snapshot)?;
        diesel::update(cycles::table.filter(cycles::uuid.eq(cycle.uuid)))
            .set(cycles::completed_at.eq(Some(*end_at)))
            .execute(conn)?;
    }

    // --- Bulk history (last 365 days) --------------------------------------
    // Populates the activity heatmap + long-range analytics. Skippable via
    // SEED_HISTORY=off for a lighter seed.
    if !matches!(
        std::env::var("SEED_HISTORY").as_deref(),
        Ok("off") | Ok("0") | Ok("false")
    ) {
        // Deterministic, key-sorted agent/requester lists for round-robin.
        let mut agents: Vec<Uuid> = Vec::new();
        let mut requesters: Vec<Uuid> = Vec::new();
        let mut keys: Vec<&String> = user_map.keys().collect();
        keys.sort();
        for k in keys {
            let (uuid, is_agent) = user_map[k];
            if is_agent {
                agents.push(uuid);
            } else {
                requesters.push(uuid);
            }
        }
        summary.history = seed_history(conn, history, now, &agents, &requesters, &category_ids)?;
    }

    Ok(summary)
}

/// Expand the history template pool across the last 365 days: one low-detail
/// ticket per generated slot, weekday-weighted, mostly closed (older than ~3
/// weeks always terminal), round-robined across requesters and agents. Drives
/// the activity heatmap (buckets by closed_at / updated_at) and long-range
/// analytics. No bodies, comments, projects, or cycles.
#[allow(clippy::too_many_arguments)]
fn seed_history(
    conn: &mut DbConnection,
    history: &DemoHistory,
    now: DateTime<Utc>,
    agents: &[Uuid],
    requesters: &[Uuid],
    category_ids: &HashMap<String, i32>,
) -> Result<usize, SeedError> {
    if history.tickets.is_empty() || agents.is_empty() || requesters.is_empty() {
        return Ok(0);
    }

    // Resolve one workflow state per category once.
    let done = repository::workflow_states::first_in_category(conn, WorkflowStateCategory::Done)?;
    let cancelled =
        repository::workflow_states::first_in_category(conn, WorkflowStateCategory::Cancelled)?;
    let active =
        repository::workflow_states::first_in_category(conn, WorkflowStateCategory::Active)?;
    let in_review =
        repository::workflow_states::first_in_category(conn, WorkflowStateCategory::InReview)?;
    let backlog =
        repository::workflow_states::first_in_category(conn, WorkflowStateCategory::Backlog)?;
    let triage =
        repository::workflow_states::first_in_category(conn, WorkflowStateCategory::Triage)?;

    let mut created = 0usize;
    let mut seq: u64 = 0;

    for d in (0..=364i64).rev() {
        let day = now - Duration::days(d);
        let weekday = day.weekday().num_days_from_monday(); // 0=Mon .. 6=Sun
        let is_weekend = weekday >= 5;

        // Tickets on this day: weekdays 1-4, weekends usually 0 (occasional 1).
        let hday = hash(d as u64);
        let count = if is_weekend {
            if hday % 5 == 0 {
                1
            } else {
                0
            }
        } else {
            1 + (hday % 4)
        };

        for k in 0..count {
            let h = hash(d as u64 * 131 + k + 1);

            let tmpl = &history.tickets[(seq as usize) % history.tickets.len()];
            let requester = requesters[(seq as usize) % requesters.len()];
            let assignee = agents[(h as usize) % agents.len()];

            // State: older than ~3 weeks is always terminal; recent is a mix.
            // ~10% of terminal tickets are cancelled.
            let recent_open = d <= 18 && h % 100 < 45;
            let (state_id, terminal) = if recent_open {
                match h % 4 {
                    0 => (active.id, false),
                    1 => (in_review.id, false),
                    2 => (triage.id, false),
                    _ => (backlog.id, false),
                }
            } else if h % 10 == 0 {
                (cancelled.id, true)
            } else {
                (done.id, true)
            };

            // created_at: spread within the working day, always <= now.
            let created_at =
                day - Duration::hours((h % 9) as i64) - Duration::minutes((h % 47) as i64);

            let new_ticket = NewTicket {
                title: tmpl.title.clone(),
                workflow_state_id: state_id,
                priority: parse_priority(&tmpl.priority),
                requester_uuid: Some(requester),
                assignee_uuid: Some(assignee),
                category_id: category_ids.get(&tmpl.category).copied(),
                submitted_via: Some("web".to_string()),
                guest_lookup_token: None,
                verification_state: None,
                origin_channel_id: None,
                triage_state: None,
                due_date: None,
                start_date: None,
                recurrence_rule: None,
                recurrence_template_id: None,
                resolution_notes: None,
                spam_suspected: false,
            };
            let ticket = repository::tickets::create_ticket(conn, new_ticket)?;

            // first response ~15min-3h after open, so SLA pills stay green.
            let first_response_at = created_at + Duration::minutes(15 + (h % 165) as i64);

            let (closed_at, updated_at) = if terminal {
                // Resolve 1h-4d later, clamped before now.
                let dur = Duration::hours(1 + (h % 96) as i64);
                let mut c = created_at + dur;
                let ceiling = now - Duration::minutes(1);
                if c > ceiling {
                    c = ceiling;
                }
                (Some(c), c)
            } else {
                // Open: last touched a bit after open (drives activity-mode).
                (None, created_at + Duration::hours((h % 6) as i64))
            };

            backdate_ticket(
                conn,
                ticket.id,
                created_at,
                updated_at,
                closed_at,
                Some(first_response_at.min(updated_at)),
            )?;

            seq += 1;
            created += 1;
        }
    }

    Ok(created)
}

/// Cheap deterministic hash (splitmix-ish) for reproducible pseudo-random
/// spread without a dependency on rand (keeps seeds reproducible).
fn hash(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
    x ^ (x >> 31)
}

/// Verify the instance is ready to seed. Returns a user uuid to attribute
/// provisioning writes to (prefers a platform admin).
fn preflight(conn: &mut DbConnection) -> Result<Uuid, SeedError> {
    let ws_count: i64 = workspaces::table.count().get_result(conn)?;
    if ws_count == 0 {
        return Err(SeedError::Precondition(
            "no workspace found. Start the stack (`make dev`) and let migrations run first."
                .to_string(),
        ));
    }

    // Guard: seeding users into a virgin instance would trip the
    // `count_users > 0` short-circuit in create_initial_admin and lock the
    // operator out of ever creating their admin. Require onboarding first.
    let user_count = repository::count_users(conn)?;
    if user_count == 0 {
        return Err(SeedError::Precondition(
            "no users yet — complete onboarding first (open the setup URL, or run \
             `nosdesk-cli admin create`), then re-run `make seed-demo`. Seeding users \
             now would block admin setup."
                .to_string(),
        ));
    }

    let already: i64 = user_emails::table
        .filter(user_emails::email.like(format!("%{DEMO_EMAIL_DOMAIN}")))
        .count()
        .get_result(conn)?;
    if already > 0 {
        return Err(SeedError::AlreadySeeded);
    }

    let admin_uuid: Uuid = users::table
        .filter(users::platform_role.eq("platform_admin"))
        .filter(users::deleted_at.is_null())
        .order(users::created_at.asc())
        .select(users::uuid)
        .first(conn)
        .optional()?
        .map(Ok)
        .unwrap_or_else(|| {
            // No platform admin (unusual) — attribute to the oldest user.
            users::table
                .filter(users::deleted_at.is_null())
                .order(users::created_at.asc())
                .select(users::uuid)
                .first(conn)
        })?;

    Ok(admin_uuid)
}

/// Insert a local (password) auth identity, mirroring
/// `services::admin_setup::create_initial_admin`.
fn insert_local_identity(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    email: &str,
    password_hash: &str,
) -> QueryResult<usize> {
    #[derive(diesel::Insertable)]
    #[diesel(table_name = backend::schema::user_auth_identities)]
    struct NewLocalAuthIdentity<'a> {
        user_uuid: Uuid,
        provider_type: &'a str,
        external_id: &'a str,
        email: Option<&'a str>,
        password_hash: Option<&'a str>,
    }
    diesel::insert_into(backend::schema::user_auth_identities::table)
        .values(&NewLocalAuthIdentity {
            user_uuid,
            provider_type: "local",
            external_id: email,
            email: Some(email),
            password_hash: Some(password_hash),
        })
        .execute(conn)
}

/// Backdate a ticket. `updated_at` is set explicitly so the
/// `diesel_set_updated_at` BEFORE-UPDATE trigger keeps our value instead of
/// stamping `now()`.
fn backdate_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
    first_response_at: Option<DateTime<Utc>>,
) -> QueryResult<usize> {
    diesel::update(tickets::table.find(ticket_id))
        .set((
            tickets::created_at.eq(created_at),
            tickets::updated_at.eq(updated_at),
            tickets::closed_at.eq(closed_at),
            tickets::first_response_at.eq(first_response_at),
        ))
        .execute(conn)
}

fn backdate_comment(
    conn: &mut DbConnection,
    comment_id: i32,
    at: DateTime<Utc>,
) -> QueryResult<usize> {
    diesel::update(comments::table.find(comment_id))
        .set((comments::created_at.eq(at), comments::updated_at.eq(at)))
        .execute(conn)
}

/// Backdate a ticket's cycle membership so the burnup chart rises across the
/// cycle rather than spiking at seed time. Rewrites both `cycle_tickets.added_at`
/// and the `cycle_ticket.added` sync event that `membership_intervals` replays.
/// A cross-partition move of the sync row re-fires the webhook-outbox trigger,
/// which is idempotent (`ON CONFLICT DO NOTHING`), so no cleanup is needed.
fn backdate_cycle_membership(
    conn: &mut DbConnection,
    cycle_id: i32,
    ticket_id: i32,
    at: DateTime<Utc>,
) -> QueryResult<usize> {
    diesel::update(
        cycle_tickets::table
            .filter(cycle_tickets::cycle_id.eq(cycle_id))
            .filter(cycle_tickets::ticket_id.eq(ticket_id)),
    )
    .set(cycle_tickets::added_at.eq(at))
    .execute(conn)?;

    let sync_ids: Vec<i64> = sync_actions::table
        .filter(sync_actions::aggregate.eq(SyncAggregate::CycleTicket))
        .filter(sync_actions::aggregate_id.eq(format!("{cycle_id}:{ticket_id}")))
        .filter(sync_actions::event_type.eq("cycle_ticket.added"))
        .select(sync_actions::sync_id)
        .load(conn)?;

    let mut n = 0;
    for sid in sync_ids {
        n += diesel::sql_query("UPDATE sync_actions SET occurred_at = $1 WHERE sync_id = $2")
            .bind::<Timestamptz, _>(at)
            .bind::<BigInt, _>(sid)
            .execute(conn)?;
    }
    Ok(n)
}

fn parse_priority(s: &str) -> TicketPriority {
    match s {
        "none" => TicketPriority::None,
        "low" => TicketPriority::Low,
        "high" => TicketPriority::High,
        "urgent" => TicketPriority::Urgent,
        _ => TicketPriority::Medium,
    }
}

fn parse_state_category(s: &str) -> WorkflowStateCategory {
    match s {
        "triage" => WorkflowStateCategory::Triage,
        "active" => WorkflowStateCategory::Active,
        "in_review" => WorkflowStateCategory::InReview,
        "done" => WorkflowStateCategory::Done,
        "cancelled" => WorkflowStateCategory::Cancelled,
        _ => WorkflowStateCategory::Backlog,
    }
}

fn print_summary(s: &Summary, data: &DemoData) {
    println!();
    println!("Demo data seeded into the bootstrap workspace:");
    println!(
        "  {} users, {} projects, {} cycles, {} devices, {} detailed tickets, {} comments",
        s.users, s.projects, s.cycles, s.devices, s.tickets, s.comments
    );
    println!(
        "  {} bulk history tickets spread across the last 365 days (for the activity graph)",
        s.history
    );
    if s.bodies_skipped > 0 {
        println!(
            "  ({} ticket bodies could not be converted and were left empty)",
            s.bodies_skipped
        );
    }
    println!();
    println!("All demo users share the password: {}", data.password);
    if let Some(member) = data.users.iter().find(|u| u.role == "member") {
        println!("  requester (logs in directly): {}", member.email);
    }
    if let Some(agent) = data.users.iter().find(|u| u.role == "agent") {
        println!("  agent:                        {}", agent.email);
    }
    println!(
        "Agent/admin accounts are prompted for a one-time MFA setup on first login\n\
         (app policy). Requesters and your existing admin sign in directly."
    );
    println!();
    println!(
        "Note: seeded tickets aren't in the running server's search index yet. As an\n\
         admin, POST /api/search/rebuild (or restart with an empty index) to index them."
    );
}
