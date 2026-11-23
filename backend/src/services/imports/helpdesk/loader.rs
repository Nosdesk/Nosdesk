//! Writes a bundle into a workspace: planned first (a dry run that writes
//! nothing), then in batches of tickets, each batch one transaction.
//!
//! Every write runs as the importing person with the actor reference
//! `import:<job>`, which the notification, rule and webhook outbox triggers
//! skip: imported history notifies nobody, fires no rule and calls no
//! webhook. Clients, search and the activity feed still see it.
//!
//! Safe to run again. The id map records every ticket, reply and file as it
//! lands, in the same transaction, so:
//! - a run that died resumes at the first ticket the map doesn't have;
//! - a rerun on a fresher bundle skips what landed and adds only the new
//!   replies (a ticket's own fields are never overwritten: the Nosdesk side
//!   may have moved on);
//! - files are stored after their batch commits and attached one by one, so
//!   a file that failed is picked up by the next run.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::QueryResult;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::bundle::{self, Body, BodyFormat, Bundle, PriorityLevel, StatusKind};
use super::people::{self, Decision, PeopleOptions, Resolved};
use crate::db::{DbConnection, Pool};
use crate::models::{
    ContentFormat, NewAttachment, NewImportedComment, NewImportedTicket, NewTag, Ticket,
    TicketPriority, WorkflowStateCategory, HELPDESK_IMPORT,
};
use crate::repository::import_source_ids::{self as idmap, Entity, Scope};
use crate::sync::actor::ActorContext;
use crate::sync::session::with_actor_context;
use crate::utils::storage::{Storage, WorkspaceScopedStorage};

/// Tickets per transaction. Large enough to amortise the transaction, small
/// enough that a failure loses little and locks are short.
pub const DEFAULT_BATCH: usize = 200;

/// Largest single file brought across. The connector caps its downloads the
/// same way; this guards bundles from elsewhere.
const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberPolicy {
    /// Keep the source's numbers when the workspace has room (no tickets of
    /// its own), else number after existing tickets.
    Auto,
    /// Keep them, or refuse to start.
    Keep,
    /// Always number after existing tickets; the source number stays on the
    /// id map and in the ticket's import note.
    AfterExisting,
}

#[derive(Debug, Clone)]
pub struct LoadOptions {
    pub job_id: Uuid,
    pub workspace_id: i32,
    /// The person running the import. Imported rows are written as theirs,
    /// under `import:<job_id>`.
    pub actor: Uuid,
    pub numbers: NumberPolicy,
    /// Source staff email → Nosdesk member email, lower-cased.
    pub agent_map: HashMap<String, String>,
    /// Create unmatched active source staff as agents (self-hosted only).
    pub create_agents: bool,
    pub batch_size: usize,
}

impl LoadOptions {
    fn actor_context(&self) -> ActorContext {
        let mut actor = ActorContext::user(self.actor, None).with_workspace(self.workspace_id);
        actor.reference = Some(format!("import:{}", self.job_id));
        actor
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("the bundle can't be read: {0}")]
    Bundle(#[from] std::io::Error),
    #[error("database: {0}")]
    Db(#[from] diesel::result::Error),
    #[error("database pool: {0}")]
    Pool(String),
    #[error("{0}")]
    Refused(String),
    #[error("ticket {ticket} failed, so its batch was rolled back: {error}. Run the import again to resume.")]
    Batch { ticket: String, error: String },
}

/// What a run would do, from the bundle and the workspace as they are.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub source: String,
    pub scope: String,
    pub tickets: u64,
    pub tickets_imported_already: u64,
    pub comments: u64,
    pub attachments: u64,
    pub attachments_missing: u64,
    pub keep_numbers: bool,
    pub numbers_reason: String,
    /// Source status → (Nosdesk state name, tickets).
    pub states: BTreeMap<String, (String, u64)>,
    /// Source type → (Nosdesk request type it matches, or None if it
    /// becomes a tag, tickets).
    pub types: BTreeMap<String, (Option<String>, u64)>,
    /// People the tickets mention, by outcome.
    pub people: BTreeMap<&'static str, u64>,
    /// Source staff and where each lands, for the operator to check.
    pub staff: Vec<(String, String, &'static str)>,
    pub notes: Vec<String>,
    /// A rough guess at the time to write, assuming ~50 tickets a second
    /// (not yet measured on a real account; files and long threads slow it).
    pub estimated_minutes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct LoadReport {
    pub tickets_written: u64,
    pub tickets_skipped: u64,
    pub comments_written: u64,
    pub attachments_written: u64,
    /// Files that weren't brought across, and why.
    pub attachment_problems: Vec<String>,
    /// Tickets not brought across again, and why (deleted here since).
    pub ticket_notes: Vec<String>,
    pub people: BTreeMap<&'static str, u64>,
}

#[derive(Debug, Clone, Default)]
pub struct LoadProgress {
    pub tickets_done: u64,
    pub tickets_total: u64,
    pub comments_written: u64,
    pub attachments_written: u64,
}

// ---------------------------------------------------------------- mapping

/// The Nosdesk state each source status lands in, resolved once per run.
struct StateMap {
    by_kind: HashMap<StatusKind, (i32, String)>,
}

impl StateMap {
    fn load(conn: &mut DbConnection) -> QueryResult<Self> {
        use crate::repository::workflow_states as ws;
        let default = ws::default_state(conn)?;
        let pick = |conn: &mut DbConnection,
                    cats: &[WorkflowStateCategory]|
         -> QueryResult<(i32, String)> {
            for c in cats {
                if let Ok(s) = ws::first_in_category(conn, *c) {
                    return Ok((s.id, s.name));
                }
            }
            Ok((default.id, default.name.clone()))
        };
        use WorkflowStateCategory as C;
        let mut by_kind = HashMap::new();
        by_kind.insert(StatusKind::New, (default.id, default.name.clone()));
        by_kind.insert(StatusKind::Open, pick(conn, &[C::Active])?);
        by_kind.insert(StatusKind::Pending, pick(conn, &[C::InReview, C::Active])?);
        by_kind.insert(StatusKind::OnHold, pick(conn, &[C::InReview, C::Active])?);
        by_kind.insert(StatusKind::Solved, pick(conn, &[C::Done])?);
        by_kind.insert(StatusKind::Closed, pick(conn, &[C::Done])?);
        Ok(StateMap { by_kind })
    }

    fn state(&self, kind: StatusKind) -> (i32, &str) {
        let (id, name) = &self.by_kind[&kind];
        (*id, name)
    }
}

fn priority(p: Option<PriorityLevel>) -> TicketPriority {
    match p {
        None => TicketPriority::None,
        Some(PriorityLevel::Low) => TicketPriority::Low,
        Some(PriorityLevel::Normal) => TicketPriority::Medium,
        Some(PriorityLevel::High) => TicketPriority::High,
        Some(PriorityLevel::Urgent) => TicketPriority::Urgent,
    }
}

/// Request types by lower-cased name.
fn categories(conn: &mut DbConnection) -> QueryResult<HashMap<String, (i32, String)>> {
    Ok(crate::repository::categories::get_all_categories(conn)?
        .into_iter()
        .map(|c| (c.name.to_lowercase(), (c.id, c.name)))
        .collect())
}

fn naive(t: DateTime<Utc>) -> NaiveDateTime {
    t.naive_utc()
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// A source body as a stored reply: sanitised the way an inbound email is,
/// classified into a render tier, with the original kept beside it.
fn comment_body(
    body: &Body,
) -> (
    String,
    ContentFormat,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    use crate::services::channels::{email_render_kind, email_sanitise};
    match body.format {
        BodyFormat::Html => {
            let clean = email_sanitise::sanitise(&body.value).html;
            let classified = email_render_kind::classify(ContentFormat::Html, &clean, None);
            (
                clean,
                ContentFormat::Html,
                Some(classified.kind.as_str().to_string()),
                Some(classified.new_content),
                Some(body.value.clone()),
                None,
            )
        }
        BodyFormat::Plain | BodyFormat::Markdown => (
            body.value.clone(),
            ContentFormat::Plaintext,
            Some("text".to_string()),
            Some(body.value.clone()),
            None,
            Some(body.value.clone()),
        ),
    }
}

fn source_title(source: &str) -> &str {
    match source {
        "zendesk" => "Zendesk",
        "freshdesk" => "Freshdesk",
        "freshservice" => "Freshservice",
        other => other,
    }
}

// ---------------------------------------------------------------- db plumbing

async fn db<T, F>(pool: &Pool, actor: ActorContext, f: F) -> Result<T, LoadError>
where
    T: Send + 'static,
    F: FnOnce(&mut DbConnection) -> Result<T, LoadError> + Send + 'static,
{
    let pool = pool.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(|e| LoadError::Pool(e.to_string()))?;
        with_actor_context(&mut conn, &actor, f)
    })
    .await
    .map_err(|e| LoadError::Pool(format!("worker: {e}")))?
}

// ---------------------------------------------------------------- plan

/// Plan the import without writing anything.
pub async fn plan(pool: &Pool, bundle: &Bundle, opts: &LoadOptions) -> Result<Plan, LoadError> {
    let manifest = bundle.read_manifest()?;
    let b = bundle.clone();
    let o = opts.clone();
    let m = manifest.clone();
    db(pool, opts.actor_context(), move |conn| {
        let scope = Scope {
            source: &m.source,
            instance: &m.source_scope,
        };
        let states = StateMap::load(conn)?;
        let cats = categories(conn)?;

        let mut plan = Plan {
            source: m.source.clone(),
            scope: m.source_scope.clone(),
            notes: m.notes.clone(),
            ..Default::default()
        };

        let mut ids = Vec::new();
        let mut numbers: HashMap<i64, String> = HashMap::new();
        let mut numbers_ok = true;
        for t in b.tickets()? {
            let t = t?;
            plan.tickets += 1;
            plan.comments += t.comments.len() as u64;
            for c in &t.comments {
                for a in &c.attachments {
                    plan.attachments += 1;
                    if !a.stored {
                        plan.attachments_missing += 1;
                    }
                }
            }
            let (_, state) = states.state(t.status);
            plan.states
                .entry(t.status_label.clone())
                .or_insert_with(|| (state.to_string(), 0))
                .1 += 1;
            if let Some(ty) = &t.ticket_type {
                let matched = cats.get(&ty.to_lowercase()).map(|(_, n)| n.clone());
                plan.types.entry(ty.clone()).or_insert((matched, 0)).1 += 1;
            }
            match t.number {
                Some(n)
                    if (1..=i64::from(i32::MAX)).contains(&n)
                        && numbers.entry(n).or_insert_with(|| t.source_id.clone())
                            == &t.source_id => {}
                _ => numbers_ok = false,
            }
            ids.push(t.source_id);
        }
        plan.tickets_imported_already =
            idmap::targets_of(conn, scope, Entity::Ticket, &ids)?.len() as u64;

        let (keep, reason) = number_decision(conn, scope, o.numbers, numbers_ok)?;
        plan.keep_numbers = keep;
        plan.numbers_reason = reason;

        let referenced = people::referenced(b.tickets()?.filter_map(Result::ok));
        let persons: Vec<bundle::Person> = b
            .people()?
            .filter_map(Result::ok)
            .filter(|p| referenced.contains(&p.source_id))
            .collect();
        let persons = first_of_each(persons);
        let popts = PeopleOptions {
            workspace_id: o.workspace_id,
            scope,
            job_id: o.job_id,
            agent_map: &o.agent_map,
            create_agents: o.create_agents,
        };
        for d in people::plan(conn, &persons, &popts)? {
            *plan.people.entry(d.outcome.label()).or_default() += 1;
            if d.person.role.is_staff() {
                plan.staff.push((
                    d.person
                        .email
                        .clone()
                        .unwrap_or_else(|| d.person.name.clone()),
                    d.target_email.clone().unwrap_or_else(|| "—".into()),
                    d.outcome.label(),
                ));
            }
        }
        let missing_people = referenced.len() as u64 - persons.len() as u64;
        if missing_people > 0 {
            plan.notes.push(format!(
                "{missing_people} people the tickets mention aren't in the bundle; their replies \
                 are credited to the placeholder author"
            ));
        }
        plan.estimated_minutes = (plan.tickets - plan.tickets_imported_already) / (50 * 60) + 1;
        Ok(plan)
    })
    .await
}

/// Whether to keep the source's ticket numbers. Only when the workspace's
/// tickets are all ones this source brought (none of its own to collide
/// with), and the source's numbers are unique and fit.
fn number_decision(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    policy: NumberPolicy,
    numbers_ok: bool,
) -> Result<(bool, String), LoadError> {
    if policy == NumberPolicy::AfterExisting {
        return Ok((false, "numbered after existing tickets, as asked".into()));
    }
    let in_workspace = crate::repository::tickets::count_tickets(conn)?;
    let ours = idmap::count(conn, scope, Entity::Ticket)?;
    let room = in_workspace == ours;
    match (room && numbers_ok, policy) {
        (true, _) => Ok((true, "the source's numbers are kept".into())),
        (false, NumberPolicy::Keep) => Err(LoadError::Refused(if !numbers_ok {
            "the source's ticket numbers aren't unique positive integers, so they can't be kept"
                .into()
        } else {
            format!(
                "the workspace already has {} tickets of its own, so the source's numbers can't \
                 be kept; run with numbers after existing",
                in_workspace - ours
            )
        })),
        (false, _) => Ok((
            false,
            if numbers_ok {
                format!("the workspace already has {} tickets, so imported tickets are numbered after them", in_workspace - ours)
            } else {
                "the source's numbers aren't usable, so tickets are numbered after existing ones"
                    .into()
            },
        )),
    }
}

// ---------------------------------------------------------------- load

/// Write the bundle into the workspace. Call [`plan`] first and show it.
pub async fn load(
    pool: &Pool,
    storage: Arc<dyn Storage>,
    bundle: &Bundle,
    opts: &LoadOptions,
    progress: &(dyn Fn(&LoadProgress) + Send + Sync),
) -> Result<LoadReport, LoadError> {
    let manifest = bundle.read_manifest()?;
    let source = manifest.source.clone();
    let instance = manifest.source_scope.clone();
    let mut report = LoadReport::default();

    // Settle numbering once, before the first batch, so a resumed run keeps
    // the choice its first batch made (the map holds only our tickets then).
    let keep_numbers = {
        let (s, i, policy) = (source.clone(), instance.clone(), opts.numbers);
        let numbers_ok = numbers_usable(bundle)?;
        db(pool, opts.actor_context(), move |conn| {
            Ok(number_decision(
                conn,
                Scope {
                    source: &s,
                    instance: &i,
                },
                policy,
                numbers_ok,
            )?
            .0)
        })
        .await?
    };

    // People first: everyone the tickets mention, in one transaction.
    let people_map: Arc<HashMap<String, Resolved>> = {
        let referenced = people::referenced(bundle.tickets()?.filter_map(Result::ok));
        let persons: Vec<bundle::Person> = bundle
            .people()?
            .filter_map(Result::ok)
            .filter(|p| referenced.contains(&p.source_id))
            .collect();
        let persons = first_of_each(persons);
        let o = opts.clone();
        let (s, i) = (source.clone(), instance.clone());
        let resolved = db(pool, opts.actor_context(), move |conn| {
            let popts = PeopleOptions {
                workspace_id: o.workspace_id,
                scope: Scope {
                    source: &s,
                    instance: &i,
                },
                job_id: o.job_id,
                agent_map: &o.agent_map,
                create_agents: o.create_agents,
            };
            let decisions: Vec<Decision> = people::plan(conn, &persons, &popts)?;
            Ok(people::apply(conn, &decisions, &popts)?)
        })
        .await?;
        for r in resolved.values() {
            *report.people.entry(r.outcome.label()).or_default() += 1;
        }
        Arc::new(resolved)
    };

    // Placeholder author for replies whose author the bundle doesn't know.
    let placeholder: Uuid = {
        let (s, i) = (source.clone(), instance.clone());
        db(pool, opts.actor_context(), move |conn| {
            let scope = Scope {
                source: &s,
                instance: &i,
            };
            let name = format!("Unknown ({})", source_title(&s));
            Ok(
                crate::repository::user_helpers::find_or_provision_requester(
                    &people::placeholder_email(scope),
                    &name,
                    conn,
                    None,
                )?
                .uuid,
            )
        })
        .await?
    };

    let total = manifest.counts.get("tickets").copied().unwrap_or(0);
    let mut prog = LoadProgress {
        tickets_total: total,
        ..Default::default()
    };
    let batch_size = opts.batch_size.max(1);
    let mut reader = bundle.tickets()?;
    loop {
        let mut batch = Vec::with_capacity(batch_size);
        for t in reader.by_ref().take(batch_size) {
            batch.push(t?);
        }
        if batch.is_empty() {
            break;
        }
        let n = batch.len() as u64;

        let written = write_batch(
            pool,
            opts,
            &source,
            &instance,
            keep_numbers,
            &people_map,
            placeholder,
            batch.clone(),
        )
        .await?;
        report.tickets_written += written.tickets;
        report.tickets_skipped += n - written.tickets;
        report.comments_written += written.comments;
        report.ticket_notes.extend(written.notes);

        let files = attach_files(
            pool,
            &storage,
            bundle,
            opts,
            &source,
            &instance,
            &people_map,
            placeholder,
            &batch,
        )
        .await?;
        report.attachments_written += files.written;
        report.attachment_problems.extend(files.problems);

        prog.tickets_done += n;
        prog.comments_written = report.comments_written;
        prog.attachments_written = report.attachments_written;
        progress(&prog);
    }
    Ok(report)
}

/// A person listed twice in a bundle is brought across once, as first listed.
fn first_of_each(people: Vec<bundle::Person>) -> Vec<bundle::Person> {
    let mut seen = HashSet::new();
    people
        .into_iter()
        .filter(|p| seen.insert(p.source_id.clone()))
        .collect()
}

fn numbers_usable(bundle: &Bundle) -> Result<bool, LoadError> {
    // Unique per ticket: the same ticket listed twice may repeat its number.
    let mut seen: HashMap<i64, String> = HashMap::new();
    for t in bundle.tickets()? {
        let t = t?;
        match t.number {
            Some(n)
                if (1..=i64::from(i32::MAX)).contains(&n)
                    && seen.entry(n).or_insert_with(|| t.source_id.clone()) == &t.source_id => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

struct BatchWritten {
    tickets: u64,
    comments: u64,
    notes: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
async fn write_batch(
    pool: &Pool,
    opts: &LoadOptions,
    source: &str,
    instance: &str,
    keep_numbers: bool,
    people: &Arc<HashMap<String, Resolved>>,
    placeholder: Uuid,
    batch: Vec<bundle::Ticket>,
) -> Result<BatchWritten, LoadError> {
    let (s, i, o, people) = (
        source.to_string(),
        instance.to_string(),
        opts.clone(),
        Arc::clone(people),
    );
    db(pool, opts.actor_context(), move |conn| {
        let scope = Scope {
            source: &s,
            instance: &i,
        };
        // One batch at a time per source and workspace, and the map read
        // under the lock, so an overlapping run skips what this one wrote.
        idmap::lock_source(conn, o.workspace_id, scope)?;
        let states = StateMap::load(conn)?;
        let cats = categories(conn)?;
        let mut tags = TagCache::load(conn)?;
        let ids: Vec<String> = batch.iter().map(|t| t.source_id.clone()).collect();
        let mut mapped = idmap::targets_of(conn, scope, Entity::Ticket, &ids)?;
        let mut live = live_tickets(conn, &mapped)?;

        let mut out = BatchWritten {
            tickets: 0,
            comments: 0,
            notes: Vec::new(),
        };
        for t in &batch {
            let label = format!(
                "{} #{}",
                source_title(&s),
                t.number.map_or(t.source_id.clone(), |n| n.to_string())
            );
            let r = match mapped.get(&t.source_id).cloned() {
                Some(target) => match target.parse::<i32>().ok().filter(|id| live.contains(id)) {
                    Some(id) => append_new_comments(conn, scope, &o, &people, placeholder, id, t)
                        .map(|c| (false, c)),
                    None => {
                        // Imported before and deleted here since: deleting
                        // it was someone's choice, so it stays deleted.
                        out.notes.push(format!(
                            "{label}: deleted here since it was imported, so not brought back"
                        ));
                        Ok((false, 0))
                    }
                },
                None => write_ticket(
                    conn,
                    scope,
                    &o,
                    keep_numbers,
                    &people,
                    placeholder,
                    &states,
                    &cats,
                    &mut tags,
                    t,
                )
                .map(|(id, c)| {
                    // A ticket listed twice in one bundle is written once;
                    // the second copy only adds replies the first lacked.
                    mapped.insert(t.source_id.clone(), id.to_string());
                    live.insert(id);
                    (true, c)
                }),
            };
            match r {
                Ok((new, comments)) => {
                    out.tickets += u64::from(new);
                    out.comments += comments;
                }
                Err(e) => {
                    return Err(LoadError::Batch {
                        ticket: label,
                        error: e.to_string(),
                    })
                }
            }
        }
        Ok(out)
    })
    .await
}

/// Which of the mapped tickets still exist here.
fn live_tickets(
    conn: &mut DbConnection,
    mapped: &HashMap<String, String>,
) -> QueryResult<HashSet<i32>> {
    use crate::schema::tickets;
    use diesel::prelude::*;
    let ids: Vec<i32> = mapped.values().filter_map(|v| v.parse().ok()).collect();
    if ids.is_empty() {
        return Ok(HashSet::new());
    }
    Ok(tickets::table
        .filter(tickets::id.eq_any(&ids))
        .select(tickets::id)
        .load::<i32>(conn)?
        .into_iter()
        .collect())
}

struct TagCache {
    by_name: HashMap<String, i32>,
}

impl TagCache {
    fn load(conn: &mut DbConnection) -> QueryResult<Self> {
        Ok(TagCache {
            by_name: crate::repository::tags::list_tags(conn, true)?
                .into_iter()
                .map(|t| (t.name.to_lowercase(), t.id))
                .collect(),
        })
    }

    fn id(&mut self, conn: &mut DbConnection, name: &str) -> QueryResult<i32> {
        let name = truncate(name, 64);
        if let Some(id) = self.by_name.get(&name.to_lowercase()) {
            return Ok(*id);
        }
        let tag = crate::repository::tags::create_tag(
            conn,
            NewTag {
                name: name.clone(),
                color: None,
                description: None,
            },
        )?;
        self.by_name.insert(name.to_lowercase(), tag.id);
        Ok(tag.id)
    }
}

#[allow(clippy::too_many_arguments)]
fn write_ticket(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    opts: &LoadOptions,
    keep_numbers: bool,
    people: &HashMap<String, Resolved>,
    placeholder: Uuid,
    states: &StateMap,
    cats: &HashMap<String, (i32, String)>,
    tags: &mut TagCache,
    t: &bundle::Ticket,
) -> Result<(i32, u64), LoadError> {
    let who = |id: &Option<String>| id.as_ref().and_then(|i| people.get(i));
    let requester = who(&t.requester);
    let assignee = who(&t.assignee);
    // Only staff can be assigned here. A source assignee without a seat is
    // named in the import note instead.
    let assignee_uuid = assignee.filter(|r| r.staff).map(|r| r.uuid);
    let (state_id, _) = states.state(t.status);
    let category = t
        .ticket_type
        .as_ref()
        .and_then(|ty| cats.get(&ty.to_lowercase()));

    // The first public reply by someone who was staff at the source, seat
    // here or not: that is when the requester was first answered.
    let first_response = t
        .comments
        .iter()
        .filter(|c| c.public)
        .find(|c| {
            c.author
                .as_ref()
                .and_then(|a| people.get(a))
                .is_some_and(|r| r.source_staff)
        })
        .map(|c| naive(c.created_at));
    let closed_at = t
        .status
        .is_finished()
        .then(|| naive(t.solved_at.unwrap_or(t.updated_at)));

    let ticket: Ticket = crate::repository::tickets::import_ticket(
        conn,
        NewImportedTicket {
            title: {
                let s = truncate(&t.subject, 255);
                if s.is_empty() {
                    "(no subject)".to_string()
                } else {
                    s
                }
            },
            workflow_state_id: state_id,
            priority: priority(t.priority),
            requester_uuid: requester.map(|r| r.uuid),
            assignee_uuid,
            category_id: category.map(|(id, _)| *id),
            submitted_via: Some(HELPDESK_IMPORT.to_string()),
            created_by: requester.map(|r| r.uuid),
            number: if keep_numbers {
                t.number.and_then(|n| i32::try_from(n).ok())
            } else {
                None
            },
            created_at: naive(t.created_at),
            updated_at: naive(t.updated_at),
            closed_at,
            closed_by: closed_at.and(assignee_uuid),
            first_response_at: first_response,
            sla_override: "none".to_string(),
        },
        scope.source,
    )?;
    idmap::record(
        conn,
        scope,
        Entity::Ticket,
        &t.source_id,
        &ticket.id.to_string(),
        opts.job_id,
        Some(naive(t.updated_at)),
    )?;

    // Tags, plus the source type when no request type matched it.
    let mut tag_ids = Vec::new();
    for name in &t.tags {
        tag_ids.push(tags.id(conn, name)?);
    }
    if let (Some(ty), None) = (&t.ticket_type, category) {
        tag_ids.push(tags.id(conn, ty)?);
    }
    if !tag_ids.is_empty() {
        tag_ids.sort_unstable();
        tag_ids.dedup();
        crate::repository::tags::set_tags_for_ticket(conn, ticket.id, &tag_ids, Some(opts.actor))?;
    }

    for w in &t.watchers {
        if let Some(r) = people.get(w) {
            if Some(r.uuid) != ticket.requester_uuid {
                crate::repository::ticket_watchers::add_watcher(conn, ticket.id, r.uuid, false)?;
            }
        }
    }

    // One internal note with what Nosdesk has no column for, dated with the
    // ticket so it sits at the top of the thread.
    let note = import_note(scope.source, t, assignee.filter(|r| !r.staff).is_some());
    write_comment(
        conn,
        scope,
        opts,
        ticket.id,
        &format!("import-note:{}", t.source_id),
        opts.actor,
        true,
        &Body {
            format: BodyFormat::Plain,
            value: note,
        },
        t.created_at,
        None,
    )?;

    let mut comments = 0;
    let mut seen = HashSet::new();
    for c in t
        .comments
        .iter()
        .filter(|c| seen.insert(c.source_id.as_str()))
    {
        write_reply(conn, scope, opts, people, placeholder, ticket.id, c)?;
        comments += 1;
    }
    Ok((ticket.id, comments))
}

/// One source reply, credited to its author's account (or the placeholder),
/// with who they were at the source kept on the comment.
fn write_reply(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    opts: &LoadOptions,
    people: &HashMap<String, Resolved>,
    placeholder: Uuid,
    ticket_id: i32,
    c: &bundle::Comment,
) -> Result<(), LoadError> {
    let resolved = c.author.as_ref().and_then(|a| people.get(a));
    let author = resolved.map_or(placeholder, |r| r.uuid);
    let source_author = SourceAuthor {
        id: c.author.as_deref(),
        name: resolved.map(|r| r.source_name.as_str()),
        staff: resolved.is_some_and(|r| r.source_staff),
    };
    write_comment(
        conn,
        scope,
        opts,
        ticket_id,
        &c.source_id,
        author,
        !c.public,
        &c.body,
        c.created_at,
        Some(source_author),
    )
}

/// Who wrote a reply at the source. Kept on the comment: a former agent
/// with no seat here is a requester-level account, and this is what still
/// says they answered as staff.
struct SourceAuthor<'a> {
    id: Option<&'a str>,
    name: Option<&'a str>,
    staff: bool,
}

fn import_note(source: &str, t: &bundle::Ticket, assignee_without_seat: bool) -> String {
    let mut lines = vec![match t.number {
        Some(n) => format!("Imported from {} #{n}.", source_title(source)),
        None => format!("Imported from {}.", source_title(source)),
    }];
    lines.push(format!("Status there: {}.", t.status_label));
    if assignee_without_seat {
        lines.push("It was assigned to someone who has no seat here, so it is unassigned.".into());
    }
    if let Some(ty) = &t.ticket_type {
        lines.push(format!("Type: {ty}."));
    }
    for (label, value) in &t.fields {
        lines.push(format!("{label}: {value}"));
    }
    lines.join("\n")
}

#[allow(clippy::too_many_arguments)]
fn write_comment(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    opts: &LoadOptions,
    ticket_id: i32,
    source_id: &str,
    author: Uuid,
    internal: bool,
    body: &Body,
    at: DateTime<Utc>,
    source_author: Option<SourceAuthor<'_>>,
) -> Result<(), LoadError> {
    let (content, format, render_kind, new_content, body_html, body_text) = comment_body(body);
    let comment = crate::repository::comments::import_comment(
        conn,
        NewImportedComment {
            content,
            ticket_id,
            user_uuid: author,
            is_internal: internal,
            content_format: format,
            render_kind,
            new_content,
            body_html,
            body_text,
            channel_metadata: Some(serde_json::json!({
                "import": {
                    "source": scope.source,
                    "scope": scope.instance,
                    "source_id": source_id,
                    "author": source_author.as_ref().and_then(|a| a.id),
                    "author_name": source_author.as_ref().and_then(|a| a.name),
                    "author_role": source_author.as_ref().map(|a| if a.staff { "staff" } else { "requester" }),
                }
            })),
            created_at: naive(at),
            updated_at: naive(at),
        },
        scope.source,
    )?;
    idmap::record(
        conn,
        scope,
        Entity::Comment,
        source_id,
        &comment.id.to_string(),
        opts.job_id,
        Some(naive(at)),
    )?;
    Ok(())
}

/// A ticket brought across before: add only the replies it hasn't seen.
fn append_new_comments(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    opts: &LoadOptions,
    people: &HashMap<String, Resolved>,
    placeholder: Uuid,
    ticket_id: i32,
    t: &bundle::Ticket,
) -> Result<u64, LoadError> {
    let ids: Vec<String> = t.comments.iter().map(|c| c.source_id.clone()).collect();
    let have = idmap::targets_of(conn, scope, Entity::Comment, &ids)?;
    let mut n = 0;
    let mut seen = HashSet::new();
    for c in t
        .comments
        .iter()
        .filter(|c| !have.contains_key(&c.source_id) && seen.insert(c.source_id.as_str()))
    {
        write_reply(conn, scope, opts, people, placeholder, ticket_id, c)?;
        n += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------- files

struct FilesWritten {
    written: u64,
    problems: Vec<String>,
}

/// Store and attach the batch's files that aren't attached yet. After the
/// batch commits, so every reply exists; one file at a time, so a failure
/// costs that file only, and the next run tries it again.
#[allow(clippy::too_many_arguments)]
async fn attach_files(
    pool: &Pool,
    storage: &Arc<dyn Storage>,
    bundle: &Bundle,
    opts: &LoadOptions,
    source: &str,
    instance: &str,
    people: &Arc<HashMap<String, Resolved>>,
    placeholder: Uuid,
    batch: &[bundle::Ticket],
) -> Result<FilesWritten, LoadError> {
    let mut out = FilesWritten {
        written: 0,
        problems: Vec::new(),
    };
    let wanted: Vec<(String, String)> = batch
        .iter()
        .flat_map(|t| {
            t.comments.iter().flat_map(|c| {
                c.attachments
                    .iter()
                    .map(|a| (c.source_id.clone(), a.source_id.clone()))
            })
        })
        .collect();
    if wanted.is_empty() {
        return Ok(out);
    }

    // Which replies landed (as which ids) and which files are attached.
    let (s, i) = (source.to_string(), instance.to_string());
    let comment_ids: Vec<String> = wanted.iter().map(|(c, _)| c.clone()).collect();
    let file_ids: Vec<String> = wanted.iter().map(|(_, a)| a.clone()).collect();
    let (comments, attached, tickets) = db(pool, opts.actor_context(), move |conn| {
        let scope = Scope {
            source: &s,
            instance: &i,
        };
        let comments = idmap::targets_of(conn, scope, Entity::Comment, &comment_ids)?;
        let attached = idmap::targets_of(conn, scope, Entity::Attachment, &file_ids)?;
        let ticket_of: HashMap<String, i32> = {
            use crate::schema::comments as c;
            use diesel::prelude::*;
            let ids: Vec<i32> = comments.values().filter_map(|v| v.parse().ok()).collect();
            c::table
                .filter(c::id.eq_any(&ids))
                .select((c::id, c::ticket_id))
                .load::<(i32, i32)>(conn)?
                .into_iter()
                .map(|(id, t)| (id.to_string(), t))
                .collect()
        };
        Ok((comments, attached, ticket_of))
    })
    .await?;

    let scoped = WorkspaceScopedStorage::arc(Arc::clone(storage), opts.workspace_id);
    for t in batch {
        for c in &t.comments {
            let Some(comment_id) = comments.get(&c.source_id) else {
                continue;
            };
            let Some(ticket_id) = tickets.get(comment_id).copied() else {
                continue;
            };
            let Ok(comment_id) = comment_id.parse::<i32>() else {
                continue;
            };
            let uploader = c
                .author
                .as_ref()
                .and_then(|a| people.get(a))
                .map_or(placeholder, |r| r.uuid);
            for a in &c.attachments {
                if attached.contains_key(&a.source_id) {
                    continue;
                }
                let label = format!(
                    "{} (on {} #{})",
                    a.file_name,
                    source_title(source),
                    t.number.unwrap_or_default()
                );
                if !a.stored {
                    out.problems.push(format!(
                        "{label}: {}",
                        a.missing_reason.as_deref().unwrap_or("not in the bundle")
                    ));
                    continue;
                }
                let path = bundle.file_path(&a.source_id);
                let meta = match std::fs::metadata(&path) {
                    Ok(m) => m,
                    Err(_) => {
                        out.problems
                            .push(format!("{label}: missing from the bundle"));
                        continue;
                    }
                };
                if meta.len() > MAX_FILE_BYTES {
                    out.problems.push(format!(
                        "{label}: larger than {} MB",
                        MAX_FILE_BYTES / 1024 / 1024
                    ));
                    continue;
                }
                let checksum = match sha256_file(&path) {
                    Ok(h) => h,
                    Err(e) => {
                        out.problems.push(format!("{label}: {e}"));
                        continue;
                    }
                };
                let name =
                    crate::utils::file_validation::FileValidator::sanitize_filename(&a.file_name)
                        .unwrap_or_else(|_| "attachment".to_string());
                let mime = a
                    .content_type
                    .clone()
                    .unwrap_or_else(|| "application/octet-stream".into());
                let stored = match scoped
                    .store_file_from_path(&path, &name, &mime, &format!("tickets/{ticket_id}"))
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        out.problems
                            .push(format!("{label}: storage refused it ({e:?})"));
                        continue;
                    }
                };
                let display =
                    crate::utils::file_validation::FileValidator::display_filename(&a.file_name)
                        .unwrap_or_else(|| name.clone());
                let (s, i, src_id, job) = (
                    source.to_string(),
                    instance.to_string(),
                    a.source_id.clone(),
                    opts.job_id,
                );
                let size = meta.len() as i64;
                let url = stored.url.clone();
                let ws_id = opts.workspace_id;
                let row = db(pool, opts.actor_context(), move |conn| {
                    let scope = Scope {
                        source: &s,
                        instance: &i,
                    };
                    idmap::lock_source(conn, ws_id, scope)?;
                    // An overlapping run may have attached it meanwhile.
                    if idmap::target_of(conn, scope, Entity::Attachment, &src_id)?.is_some() {
                        return Ok(false);
                    }
                    let att = crate::repository::comments::create_attachment(
                        conn,
                        NewAttachment {
                            url,
                            name: display,
                            file_size: Some(size),
                            mime_type: Some(mime),
                            checksum: Some(checksum),
                            comment_id: Some(comment_id),
                            uploaded_by: Some(uploader),
                            transcription: None,
                        },
                    )?;
                    idmap::record(
                        conn,
                        scope,
                        Entity::Attachment,
                        &src_id,
                        &att.id.to_string(),
                        job,
                        None,
                    )?;
                    Ok(true)
                })
                .await;
                match row {
                    Ok(true) => out.written += 1,
                    Ok(false) => {
                        let _ = scoped.delete_file(&stored.path).await;
                    }
                    Err(e) => {
                        // The row didn't land: don't leave the file behind.
                        let _ = scoped.delete_file(&stored.path).await;
                        out.problems.push(format!("{label}: {e}"));
                    }
                }
            }
        }
    }
    Ok(out)
}

fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorities_map_with_normal_as_medium() {
        assert_eq!(
            priority(Some(PriorityLevel::Normal)),
            TicketPriority::Medium
        );
        assert_eq!(priority(None), TicketPriority::None);
        assert_eq!(
            priority(Some(PriorityLevel::Urgent)),
            TicketPriority::Urgent
        );
    }

    #[test]
    fn titles_are_cut_to_the_column() {
        let long = "x".repeat(300);
        assert_eq!(truncate(&long, 255).chars().count(), 255);
        assert_eq!(truncate("  short  ", 255), "short");
    }

    #[test]
    fn html_bodies_are_sanitised_and_keep_the_original() {
        let (content, format, kind, _, raw, _) = comment_body(&Body {
            format: BodyFormat::Html,
            value: "<p>Hi</p><script>alert(1)</script>".into(),
        });
        assert_eq!(format, ContentFormat::Html);
        assert!(!content.contains("<script"));
        assert!(kind.is_some());
        assert_eq!(raw.as_deref(), Some("<p>Hi</p><script>alert(1)</script>"));
    }
}
