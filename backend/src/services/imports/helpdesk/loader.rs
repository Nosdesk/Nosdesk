//! Writes a bundle into a workspace: planned first (a dry run that writes
//! nothing), then in batches of tickets, each batch one transaction.
//!
//! Every write is the import's own: a system actor with the reference
//! `import:<job>`, which the notification, rule and webhook outbox triggers
//! skip, so imported history notifies nobody, fires no rule and calls no
//! webhook. Clients, search and the activity feed still see it. The run's
//! `import_jobs` row (type `helpdesk`) records who ran it, into which
//! workspace, from which source, and how it ended.
//!
//! Safe to run again. The id map records every ticket, reply and file as it
//! lands, in the same transaction, so:
//! - a run that died resumes at the first ticket the map doesn't have;
//! - one ticket that can't be written is rolled back alone (a savepoint),
//!   reported, and tried again by the next run; the rest of its batch lands;
//! - a rerun on a fresher bundle skips what landed and adds only the new
//!   replies. During a cutover it also brings a ticket's status, assignee
//!   and closing up to date from the source, but only while nobody here has
//!   changed the ticket since the import last wrote it; one changed here is
//!   left alone and reported;
//! - files are stored after their batch commits and attached one by one, so
//!   a file that failed is picked up by the next run.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::{Connection, QueryResult};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::bundle::{self, Body, BodyFormat, Bundle, PriorityLevel, StatusKind};
use super::people::{self, PeopleOptions, Resolved};
use crate::db::{DbConnection, Pool};
use crate::models::{
    ContentFormat, NewAttachment, NewImportedComment, NewImportedTicket, NewTag, Ticket,
    TicketPriority, WorkflowStateCategory, HELPDESK_IMPORT,
};
use crate::repository::import_source_ids::{self as idmap, Entity, Scope};
use crate::sync::actor::ActorContext;
use crate::sync::session::with_actor_context;
use crate::utils::storage::{Storage, WorkspaceScopedStorage};

/// `import_jobs.job_type` of a helpdesk import's run.
pub const JOB_TYPE: &str = "helpdesk";

/// Tickets per transaction. Large enough to amortise the transaction, small
/// enough that a failure loses little and locks are short.
pub const DEFAULT_BATCH: usize = 200;

/// People per transaction, about: people who share an address are never
/// split across two.
pub const PEOPLE_BATCH: usize = 500;

/// The highest source number kept. A workspace's numbers continue from its
/// highest, so keeping one near the top of the column's range would leave
/// room for few tickets after it.
const MAX_KEPT_NUMBER: i64 = 1_000_000_000;

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
    /// The owner or admin running the import: the job's creator, and the
    /// author of each ticket's import note. The rows themselves are written
    /// by the import (a system actor under `import:<job_id>`), not as theirs.
    pub actor: Uuid,
    pub numbers: NumberPolicy,
    /// Source staff email → Nosdesk member email, lower-cased.
    pub agent_map: HashMap<String, String>,
    /// Create unmatched active source staff as agents (self-hosted only).
    pub create_agents: bool,
    /// Write even though the tickets name people the bundle doesn't hold
    /// and nothing says they are gone at the source. Their tickets and
    /// replies go to the placeholder author for good.
    pub allow_missing_people: bool,
    pub batch_size: usize,
    /// People per transaction ([`PEOPLE_BATCH`]).
    pub people_batch_size: usize,
    /// Stops the run between tickets and between files (the CLI cancels it
    /// on Ctrl-C), so nothing is left half written.
    pub cancel: CancellationToken,
}

impl LoadOptions {
    fn actor_context(&self) -> ActorContext {
        ActorContext::system(format!("import:{}", self.job_id)).with_workspace(self.workspace_id)
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
    #[error("stopped; run the import again to carry on from here")]
    Cancelled,
}

/// A run that stopped short: why, and what it did before.
#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct LoadFailure {
    pub error: LoadError,
    pub report: Box<LoadReport>,
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
    /// People the tickets name that the bundle doesn't hold, for a reason
    /// that needs the operator's say-so (or, on an API bundle, gone at the
    /// source).
    pub missing_people: Option<MissingPeople>,
    /// People the API fill asked the source for that it didn't return:
    /// gone there, so no say-so is needed.
    pub gone_people: Option<MissingPeople>,
    /// Why a run with these options would refuse to write, when it would.
    pub refused: Option<String>,
    /// What the bundle was read from, when it says.
    pub inputs: Option<bundle::InputSet>,
    /// What the API fill fetched, when one ran.
    pub fill: Option<bundle::Fill>,
    pub people_detail: PeopleDetail,
    pub notes: Vec<String>,
    /// A rough guess at the time to write, assuming ~50 tickets a second
    /// (not yet measured on a real account; files and long threads slow it).
    pub estimated_minutes: u64,
}

/// What the plan says about the people beyond each one's outcome.
#[derive(Debug, Clone, Default)]
pub struct PeopleDetail {
    /// People the tickets mention.
    pub mentioned: u64,
    /// Deleted at the source: their history keeps their name, no address
    /// is stored.
    pub deleted: u64,
    /// Addresses more than one source person used, and those people.
    pub shared_addresses: u64,
    pub shared_address_people: u64,
    /// Staff with partial access at the source, given no seat unless mapped.
    pub limited_staff: u64,
    /// Agents on a custom role at the source, by address or name.
    pub custom_role_agents: Vec<String>,
    /// Organizations someone brought across belongs to.
    pub organizations: u64,
    /// Of those, the ones with domain names, which aren't carried.
    pub organizations_with_domains: u64,
    /// People whose organization the bundle doesn't hold: no organization.
    pub organizations_missing: u64,
    pub profile: people::ProfileTally,
    /// Primary phones not carried because they are shared numbers.
    pub shared_phones: u64,
    /// People brought across before whom the source has changed since.
    pub changed_at_source: Vec<String>,
    pub sharing: Option<SharingNote>,
}

/// What portal sharing by domain would show colleagues that the source
/// didn't: imported tickets from organizations that didn't share them (or
/// none), raised by requesters at a domain that isn't free mail.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharingNote {
    /// Whether the workspace shares by domain now.
    pub on: bool,
    pub domains: u64,
    pub tickets: u64,
}

#[derive(Debug, Clone, Default)]
pub struct LoadReport {
    pub tickets_written: u64,
    pub tickets_skipped: u64,
    /// Tickets imported before whose status, assignee or closing were
    /// brought up to date from the source.
    pub tickets_updated: u64,
    /// Tickets a stopped run didn't reach. The next run writes them.
    pub tickets_not_attempted: u64,
    pub comments_written: u64,
    pub attachments_written: u64,
    /// Files that weren't brought across, and why.
    pub attachment_problems: Vec<String>,
    /// Tickets not brought across again, or whose new replies weren't
    /// added, and why (deleted or merged here since).
    pub ticket_notes: Vec<String>,
    /// Tickets that couldn't be written, and why. The next run tries them
    /// again.
    pub tickets_failed: Vec<String>,
    /// Tickets changed here since they were imported, so the source's newer
    /// status and assignee weren't applied.
    pub tickets_changed_here: Vec<String>,
    /// Tickets whose source number was taken here, and the number they got.
    pub numbers_moved: Vec<String>,
    /// Lines of the bundle that couldn't be read, and were skipped.
    pub unreadable_lines: u64,
    /// People who couldn't be brought across as planned, and what
    /// happened instead.
    pub people_problems: Vec<String>,
    pub people: BTreeMap<&'static str, u64>,
    /// People the tickets name that the bundle doesn't hold, credited to
    /// the placeholder author.
    pub missing_people: Option<MissingPeople>,
    /// People the API fill found gone at the source, credited to the
    /// placeholder author.
    pub gone_people: Option<MissingPeople>,
    /// What the profile fill wrote.
    pub profile: people::ProfileTally,
    /// People whose organization or phone couldn't be written, and why.
    pub profile_problems: Vec<String>,
    /// People brought across before whom the source has changed since,
    /// left as they are.
    pub changed_at_source: Vec<String>,
}

/// Why a bundle lacks someone its tickets name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Absence {
    /// The bundle was read from an export that didn't carry them (a
    /// tickets-only export carries nobody).
    NotInExport,
    /// The bundle was fetched from the source's API, which returned every
    /// user it has: they were deleted and purged there (Zendesk purges
    /// deleted end users after 30 days), or merged away.
    GoneAtSource,
    /// The bundle doesn't say where its people came from: it was written
    /// before manifests recorded that. Treated like an export.
    NotInBundle,
    /// The bundle was fetched from the API, but some of its people lines
    /// can't be read, so someone absent may be on one of them: not
    /// confirmed gone.
    Unreadable,
}

impl Absence {
    /// `people_unreadable` is whether any line of people.jsonl couldn't be
    /// read.
    pub fn of(source: Option<bundle::PeopleSource>, people_unreadable: bool) -> Self {
        match source {
            Some(bundle::PeopleSource::Export) => Absence::NotInExport,
            Some(bundle::PeopleSource::Api) if people_unreadable => Absence::Unreadable,
            Some(bundle::PeopleSource::Api) => Absence::GoneAtSource,
            None => Absence::NotInBundle,
        }
    }

    pub fn label(self, source: &str) -> String {
        match self {
            Absence::NotInExport => "not in the export".into(),
            Absence::GoneAtSource => format!("gone at {}", source_title(source)),
            Absence::NotInBundle => "not in the bundle".into(),
            Absence::Unreadable => "unreadable in the bundle".into(),
        }
    }

    /// Whether writing without them needs the operator's say-so. People the
    /// source has confirmed gone don't: almost every mature account has
    /// some, and a flag everyone must pass would mean nothing.
    pub fn needs_consent(self) -> bool {
        self != Absence::GoneAtSource
    }
}

/// People the tickets name that the bundle doesn't hold, and how much of
/// the history is theirs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPeople {
    pub people: u64,
    /// Tickets naming at least one of them in any role, each counted once.
    pub tickets: u64,
    /// Replies one of them wrote.
    pub replies: u64,
    pub absence: Absence,
    /// The absence in words ("not in the export"), for the plan, the
    /// report and each affected ticket's import note.
    pub label: String,
}

impl MissingPeople {
    /// Why a run refuses to write, or None when it may go ahead: always
    /// for people the source confirms gone, otherwise only when `allow`.
    pub fn refusal(&self, allow: bool) -> Option<String> {
        if self.people == 0 || allow || !self.absence.needs_consent() {
            return None;
        }
        // The bundle is complete, so a plain rerun reuses it: every fix
        // needs --refetch to read the source again.
        let fix = match self.absence {
            Absence::NotInBundle => {
                "The bundle predates recording where its people came from. To bring them \
                 across, run again with --refetch and either a credential, or a zip that also \
                 holds Zendesk's users export."
            }
            Absence::Unreadable => {
                "Some lines of the bundle's people.jsonl can't be read, so they aren't \
                 confirmed gone at the source. To bring them across, run again with \
                 --refetch."
            }
            _ => {
                "To bring them across, run again with --refetch and either a zip that also \
                 holds Zendesk's users export, or a credential in place of --export-zip."
            }
        };
        Some(format!(
            "the tickets name {} the bundle doesn't hold ({}), on {} and {}. Imported now, \
             the tickets they requested and the replies they wrote would be credited to the \
             placeholder author for good, tickets assigned to them would be unassigned, and \
             they would watch nothing. {fix} To import without them, pass --allow-missing-people: each affected \
             ticket's import note then names its requester and assignee by source id.",
            counted(self.people, "person", "people"),
            self.label,
            counted(self.tickets, "ticket", "tickets"),
            counted(self.replies, "reply", "replies"),
        ))
    }
}

/// How someone the bundle lacks is named in a ticket's import note: gone at
/// the source when the fill asked for them and it didn't return them,
/// otherwise as the bundle's people source says.
#[derive(Debug, Clone)]
pub struct Absent {
    pub label: String,
    pub gone: HashSet<String>,
    pub gone_label: String,
}

impl Absent {
    fn of(&self, id: &str) -> &str {
        if self.gone.contains(id) {
            &self.gone_label
        } else {
            &self.label
        }
    }
}

/// `n` with the noun that agrees with it.
pub(super) fn counted(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[derive(Debug, Clone, Default)]
pub struct LoadProgress {
    pub people_done: u64,
    pub people_total: u64,
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

/// What one read of the bundle's tickets found, without the database.
#[derive(Debug, Default)]
struct Scan {
    /// Each ticket's source id, once, in bundle order.
    ids: Vec<String>,
    /// Lines that listed a ticket again.
    repeated: u64,
    /// Lines that couldn't be read.
    unreadable: u64,
    /// Every ticket carries a number, unique to it, between 1 and
    /// [`MAX_KEPT_NUMBER`].
    numbers_ok: bool,
    /// The highest number, when they are.
    top: Option<i32>,
    /// Everyone the tickets mention, by source id.
    mentioned: HashSet<String>,
}

/// Reads tickets.jsonl through `each`, skipping (and counting) lines that
/// don't read as a ticket. Any other read error stops it.
fn each_ticket(
    bundle: &Bundle,
    unreadable: &mut u64,
    mut each: impl FnMut(bundle::Ticket),
) -> Result<(), LoadError> {
    for t in bundle.tickets()? {
        match t {
            Ok(t) => each(t),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => *unreadable += 1,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn scan(bundle: &Bundle, mut also: impl FnMut(&bundle::Ticket)) -> Result<Scan, LoadError> {
    let mut out = Scan {
        numbers_ok: true,
        ..Default::default()
    };
    let mut seen = HashSet::new();
    let mut numbers: HashMap<i64, String> = HashMap::new();
    let mut unreadable = 0;
    each_ticket(bundle, &mut unreadable, |t| {
        also(&t);
        people::mentioned(&t, &mut out.mentioned);
        // Unique per ticket: the same ticket listed twice may repeat it.
        match t.number {
            Some(n)
                if (1..=MAX_KEPT_NUMBER).contains(&n)
                    && numbers.entry(n).or_insert_with(|| t.source_id.clone()) == &t.source_id =>
            {
                let n = i32::try_from(n).unwrap_or(i32::MAX);
                out.top = Some(out.top.map_or(n, |top| top.max(n)));
            }
            _ => out.numbers_ok = false,
        }
        if seen.insert(t.source_id.clone()) {
            out.ids.push(t.source_id);
        } else {
            out.repeated += 1;
        }
    })?;
    out.unreadable = unreadable;
    if !out.numbers_ok {
        out.top = None;
    }
    Ok(out)
}

/// The bundle's people the tickets mention, each once, as first listed,
/// with the number of lines of people.jsonl that couldn't be read.
fn mentioned_people(
    bundle: &Bundle,
    mentioned: &HashSet<String>,
) -> Result<(Vec<bundle::Person>, u64), LoadError> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut unreadable = 0;
    for p in bundle.people()? {
        match p {
            Ok(mut p) => {
                if mentioned.contains(&p.source_id) && seen.insert(p.source_id.clone()) {
                    // A bundle from another tool may carry NULs, which
                    // Postgres text can't hold.
                    p.name = p.name.replace('\0', "");
                    p.email = p.email.map(|e| e.replace('\0', ""));
                    p.organization = p.organization.map(|o| o.replace('\0', ""));
                    p.phone = p.phone.map(|n| n.replace('\0', ""));
                    out.push(p);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => unreadable += 1,
            Err(e) => return Err(e.into()),
        }
    }
    Ok((out, unreadable))
}

/// The people the tickets name and `persons` lacks, and the tickets and
/// replies that are theirs (each ticket as first listed, as the plan counts
/// it), in two groups: those the bundle's people source leaves unexplained
/// (first), and those the API fill asked for and the source didn't return,
/// gone there (second). None for a group with nobody in it.
/// `people_unreadable` is the number of lines of people.jsonl that couldn't
/// be read, any of which may be one of them.
fn missing_people(
    bundle: &Bundle,
    manifest: &bundle::Manifest,
    mentioned: &HashSet<String>,
    persons: &[bundle::Person],
    people_unreadable: u64,
) -> Result<(Option<MissingPeople>, Option<MissingPeople>), LoadError> {
    let held: HashSet<&str> = persons.iter().map(|p| p.source_id.as_str()).collect();
    let gone_at_source: HashSet<&str> = manifest
        .fill
        .iter()
        .flat_map(|f| f.gone.iter().map(String::as_str))
        .collect();
    let (mut missing, mut gone) = (HashSet::new(), HashSet::new());
    for id in mentioned.iter().map(String::as_str) {
        if held.contains(id) {
            continue;
        }
        if gone_at_source.contains(id) {
            gone.insert(id);
        } else {
            missing.insert(id);
        }
    }
    if missing.is_empty() && gone.is_empty() {
        return Ok((None, None));
    }
    // (tickets, replies) for each group.
    let mut touched = [(0u64, 0u64); 2];
    let mut seen = HashSet::new();
    let mut unreadable = 0;
    each_ticket(bundle, &mut unreadable, |t| {
        if !seen.insert(t.source_id.clone()) {
            return;
        }
        let mut named = HashSet::new();
        people::mentioned(&t, &mut named);
        for (i, set) in [&missing, &gone].into_iter().enumerate() {
            if named.iter().any(|id| set.contains(id.as_str())) {
                touched[i].0 += 1;
            }
            let mut written = HashSet::new();
            touched[i].1 += t
                .comments
                .iter()
                .filter(|c| written.insert(c.source_id.as_str()))
                .filter(|c| c.author.as_deref().is_some_and(|a| set.contains(a)))
                .count() as u64;
        }
    })?;
    let absence = Absence::of(manifest.people_source, people_unreadable > 0);
    let group = |n: usize, (tickets, replies): (u64, u64), absence: Absence| {
        (n > 0).then(|| MissingPeople {
            people: n as u64,
            tickets,
            replies,
            absence,
            label: absence.label(&manifest.source),
        })
    };
    Ok((
        group(missing.len(), touched[0], absence),
        group(gone.len(), touched[1], Absence::GoneAtSource),
    ))
}

/// Why `--create-agents` can't be used on this bundle, when it can't: one
/// written before people records said who was limited staff at the source
/// reads them all as full agents, and would seat them.
fn create_agents_refusal(manifest: &bundle::Manifest, create_agents: bool) -> Option<String> {
    (create_agents && manifest.person_detail < bundle::PERSON_DETAIL).then(|| {
        "this bundle predates recording who was limited staff at the source (light agents, \
         chat-only agents, contributors), so --create-agents would make them agents here. Run \
         again with --refetch to read the source again, or without --create-agents."
            .to_string()
    })
}

/// Plan the import without writing anything.
pub async fn plan(pool: &Pool, bundle: &Bundle, opts: &LoadOptions) -> Result<Plan, LoadError> {
    let manifest = bundle.read_manifest()?;
    let b = bundle.clone();
    let o = opts.clone();
    let m = manifest.clone();
    db(pool, opts.actor_context(), move |conn| {
        let scope = Scope {
            workspace_id: o.workspace_id,
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

        let mut counted = HashSet::new();
        // Each ticket's requester and organization, for what portal sharing
        // by domain would show.
        let mut raised: Vec<(String, Option<String>)> = Vec::new();
        let found = scan(&b, |t| {
            if !counted.insert(t.source_id.clone()) {
                return;
            }
            if let Some(r) = &t.requester {
                raised.push((r.clone(), t.organization_id.clone()));
            }
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
        })?;
        plan.tickets = found.ids.len() as u64;
        plan.tickets_imported_already =
            idmap::targets_of(conn, scope, Entity::Ticket, &found.ids)?.len() as u64;
        if found.repeated > 0 {
            plan.notes.push(format!(
                "{} tickets are listed more than once; each is written once",
                found.repeated
            ));
        }

        let (keep, reason) = number_decision(conn, scope, o.numbers, found.numbers_ok)?;
        plan.keep_numbers = keep;
        plan.numbers_reason = reason;

        let (persons, bad_people) = mentioned_people(&b, &found.mentioned)?;
        let unreadable = found.unreadable + bad_people;
        if unreadable > 0 {
            plan.notes.push(format!(
                "{unreadable} lines of the bundle can't be read and will be skipped"
            ));
        }
        let (missing, gone) = missing_people(&b, &m, &found.mentioned, &persons, bad_people)?;
        plan.missing_people = missing;
        plan.gone_people = gone;
        plan.inputs = m.inputs.clone();
        plan.fill = m.fill.clone();
        let organizations = b.organizations()?;
        plan.people_detail = people_detail(&persons, &organizations, found.mentioned.len());
        plan.people_detail.sharing = sharing_note(conn, &raised, &persons, &organizations)?;

        let groups = people::group_by_address(persons);
        let popts = PeopleOptions {
            scope,
            job_id: o.job_id,
            agent_map: &o.agent_map,
            create_agents: o.create_agents,
        };
        let people_plan = people::plan(conn, &groups, &popts)?;
        plan.notes.extend(people_plan.warnings);
        plan.people_detail.profile = people_plan.profile;
        plan.people_detail.changed_at_source = people_plan.changed_at_source;
        for d in people_plan.decisions {
            *plan.people.entry(d.outcome.label()).or_default() += 1;
            if d.person.role.is_staff() {
                plan.staff.push((
                    d.person
                        .email
                        .clone()
                        .unwrap_or_else(|| d.person.name.clone()),
                    d.target_email.clone().unwrap_or_else(|| "none".into()),
                    d.outcome.label(),
                ));
            }
        }
        plan.refused = plan
            .missing_people
            .as_ref()
            .and_then(|missing| missing.refusal(o.allow_missing_people))
            .or_else(|| create_agents_refusal(&m, o.create_agents));
        plan.estimated_minutes = (plan.tickets - plan.tickets_imported_already) / (50 * 60) + 1;
        Ok(plan)
    })
    .await
}

/// The plan's counts of the people beyond their outcomes, from the bundle.
fn people_detail(
    persons: &[bundle::Person],
    organizations: &[bundle::Organization],
    mentioned: usize,
) -> PeopleDetail {
    let orgs: HashMap<&str, &bundle::Organization> = organizations
        .iter()
        .filter(|o| !o.deleted)
        .map(|o| (o.source_id.as_str(), o))
        .collect();
    let mut detail = PeopleDetail {
        mentioned: mentioned as u64,
        ..Default::default()
    };
    let mut by_address: HashMap<String, u64> = HashMap::new();
    let mut belonged: HashSet<&str> = HashSet::new();
    for p in persons {
        if let Some(e) = &p.email {
            *by_address.entry(e.to_lowercase()).or_default() += 1;
        }
        detail.deleted += u64::from(p.deleted);
        if p.deleted {
            continue;
        }
        detail.limited_staff += u64::from(p.role.is_staff() && p.limited_staff);
        detail.shared_phones += u64::from(p.shared_phone);
        if p.custom_role {
            detail
                .custom_role_agents
                .push(p.email.clone().unwrap_or_else(|| p.name.clone()));
        }
        match p.organization_id.as_deref() {
            Some(id) if orgs.contains_key(id) => {
                belonged.insert(id);
            }
            // Named, and the bundle doesn't hold it (or not alive): none.
            Some(_) if p.organization.is_none() => detail.organizations_missing += 1,
            _ => {}
        }
    }
    for n in by_address.values().filter(|n| **n > 1) {
        detail.shared_addresses += 1;
        detail.shared_address_people += n;
    }
    detail.organizations = belonged.len() as u64;
    detail.organizations_with_domains = belonged
        .iter()
        .filter(|id| orgs.get(*id).is_some_and(|o| !o.domain_names.is_empty()))
        .count() as u64;
    detail
}

/// What portal sharing by domain would show colleagues that the source
/// didn't. Sharing keys on a requester's verified address, which imported
/// requesters prove only by signing in, so this is what would come to be
/// shared as they do. None when nothing would.
fn sharing_note(
    conn: &mut DbConnection,
    raised: &[(String, Option<String>)],
    persons: &[bundle::Person],
    organizations: &[bundle::Organization],
) -> Result<Option<SharingNote>, LoadError> {
    let shared: HashSet<&str> = organizations
        .iter()
        .filter(|o| o.shared_tickets && !o.deleted)
        .map(|o| o.source_id.as_str())
        .collect();
    let domain_of: HashMap<&str, String> = persons
        .iter()
        .filter(|p| !p.deleted)
        .filter_map(|p| {
            let email = p.email.as_deref()?;
            let domain = email.rsplit_once('@')?.1.to_lowercase();
            (!crate::utils::free_mail::is_free_mail(email))
                .then_some((p.source_id.as_str(), domain))
        })
        .collect();
    let mut domains = HashSet::new();
    let mut tickets = 0u64;
    for (requester, org) in raised {
        if org.as_deref().is_some_and(|o| shared.contains(o)) {
            continue;
        }
        if let Some(d) = domain_of.get(requester.as_str()) {
            domains.insert(d.clone());
            tickets += 1;
        }
    }
    if tickets == 0 {
        return Ok(None);
    }
    let on = crate::repository::site_settings::get_site_settings(conn)?.portal_share_by_domain;
    Ok(Some(SharingNote {
        on,
        domains: domains.len() as u64,
        tickets,
    }))
}

/// Whether to keep the source's ticket numbers. Only when every ticket in the
/// workspace is one this source brought (none of its own, or another
/// import's, to collide with, however many of its own were deleted since),
/// and the source's numbers are unique and fit.
fn number_decision(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    policy: NumberPolicy,
    numbers_ok: bool,
) -> Result<(bool, String), LoadError> {
    if policy == NumberPolicy::AfterExisting {
        return Ok((false, "numbered after existing tickets, as asked".into()));
    }
    let others = idmap::other_tickets(conn, scope)?;
    let room = others == 0;
    match (room && numbers_ok, policy) {
        (true, _) => Ok((true, "the source's numbers are kept".into())),
        (false, NumberPolicy::Keep) => Err(LoadError::Refused(if !numbers_ok {
            format!(
                "the source's ticket numbers aren't unique whole numbers from 1 to \
                 {MAX_KEPT_NUMBER}, so they can't be kept"
            )
        } else {
            format!(
                "the workspace already has {others} tickets of its own, so the source's numbers \
                 can't be kept; run with numbers after existing"
            )
        })),
        (false, _) => Ok((
            false,
            if numbers_ok {
                format!("the workspace already has {others} tickets, so imported tickets are numbered after them")
            } else {
                "the source's numbers aren't usable, so tickets are numbered after existing ones"
                    .into()
            },
        )),
    }
}

// ---------------------------------------------------------------- load

/// Write the bundle into the workspace. Call [`plan`] first and show it.
/// A run that stops short says what it did before in [`LoadFailure::report`].
pub async fn load(
    pool: &Pool,
    storage: Arc<dyn Storage>,
    bundle: &Bundle,
    opts: &LoadOptions,
    progress: &(dyn Fn(&LoadProgress) + Send + Sync),
) -> Result<LoadReport, LoadFailure> {
    let mut report = LoadReport::default();
    // Read and checked before anything is written, the job row included: a
    // refused run leaves no trace in the workspace.
    let opened = match prepare(bundle) {
        Ok(prepared) => {
            report.missing_people = prepared.missing.clone();
            report.gone_people = prepared.gone.clone();
            let refusal = prepared
                .missing
                .as_ref()
                .and_then(|m| m.refusal(opts.allow_missing_people))
                .or_else(|| create_agents_refusal(&prepared.manifest, opts.create_agents));
            match refusal {
                Some(why) => Err(LoadError::Refused(why)),
                // Stopped while the bundle was read: nothing written yet.
                None if opts.cancel.is_cancelled() => Err(LoadError::Cancelled),
                None => open_job(pool, opts, &prepared.manifest)
                    .await
                    .map(|()| prepared),
            }
        }
        Err(e) => Err(e),
    };
    let prepared = match opened {
        Ok(p) => p,
        Err(error) => {
            return Err(LoadFailure {
                error,
                report: Box::new(report),
            })
        }
    };
    let result = write_all(pool, storage, bundle, opts, progress, prepared, &mut report).await;
    let closed = close_job(pool, opts, &report, result.as_ref().err()).await;
    match result.and(closed) {
        Ok(()) => Ok(report),
        Err(error) => Err(LoadFailure {
            error,
            report: Box::new(report),
        }),
    }
}

/// What a run reads of the bundle before it writes anything.
struct Prepared {
    manifest: bundle::Manifest,
    found: Scan,
    /// The bundle's people the tickets mention, grouped by address.
    groups: Vec<people::AddressGroup>,
    people_total: u64,
    /// Lines of people.jsonl that couldn't be read.
    bad_people: u64,
    missing: Option<MissingPeople>,
    gone: Option<MissingPeople>,
}

fn prepare(bundle: &Bundle) -> Result<Prepared, LoadError> {
    let manifest = bundle.read_manifest()?;
    let found = scan(bundle, |_| {})?;
    let (persons, bad_people) = mentioned_people(bundle, &found.mentioned)?;
    let (missing, gone) =
        missing_people(bundle, &manifest, &found.mentioned, &persons, bad_people)?;
    let people_total = persons.len() as u64;
    Ok(Prepared {
        manifest,
        found,
        groups: people::group_by_address(persons),
        people_total,
        bad_people,
        missing,
        gone,
    })
}

/// Open the run's job row: who ran it, from which source, into which
/// workspace. `records_committed` is set from the start, so the job sweep
/// never takes a running import for an abandoned upload.
async fn open_job(
    pool: &Pool,
    opts: &LoadOptions,
    manifest: &bundle::Manifest,
) -> Result<(), LoadError> {
    let (job, actor) = (opts.job_id, opts.actor);
    let filename = truncate(
        &format!(
            "{} ({})",
            source_title(&manifest.source),
            manifest.source_scope
        ),
        255,
    );
    let options = serde_json::json!({
        "source": manifest.source,
        "scope": manifest.source_scope,
        "numbers": format!("{:?}", opts.numbers),
        "create_agents": opts.create_agents,
        "mapped_agents": opts.agent_map.len(),
        "allow_missing_people": opts.allow_missing_people,
        "inputs": manifest.inputs.as_ref().map(|i| serde_json::json!({
            "zips": i.names(),
            "credential": i.credential,
            "api_fill": i.api_fill,
        })),
    });
    db(pool, opts.actor_context(), move |conn| {
        use crate::models::{ImportJobUpdate, NewImportJob};
        crate::repository::imports::create(
            conn,
            NewImportJob {
                id: job,
                job_type: JOB_TYPE.to_string(),
                filename,
                file_path: String::new(),
                created_by: Some(actor),
                upload_of: None,
                restore_of: None,
            },
        )?;
        crate::repository::imports::update(
            conn,
            job,
            ImportJobUpdate {
                status: Some("committing".into()),
                records_committed: Some(Some(0)),
                options: Some(Some(options)),
                ..Default::default()
            },
        )?;
        Ok(())
    })
    .await
}

/// How many tickets the run has written, on its job row.
async fn job_progress(pool: &Pool, opts: &LoadOptions, written: u64) -> Result<(), LoadError> {
    let job = opts.job_id;
    db(pool, opts.actor_context(), move |conn| {
        crate::repository::imports::update(
            conn,
            job,
            crate::models::ImportJobUpdate {
                records_committed: Some(Some(i32::try_from(written).unwrap_or(i32::MAX))),
                ..Default::default()
            },
        )?;
        Ok(())
    })
    .await
}

/// Close the run's job row: done, or failed with the reason, and what it
/// wrote either way.
async fn close_job(
    pool: &Pool,
    opts: &LoadOptions,
    report: &LoadReport,
    error: Option<&LoadError>,
) -> Result<(), LoadError> {
    let job = opts.job_id;
    let summary = serde_json::json!({
        "tickets_written": report.tickets_written,
        "tickets_skipped": report.tickets_skipped,
        "tickets_updated": report.tickets_updated,
        "tickets_failed": report.tickets_failed.len(),
        "tickets_changed_here": report.tickets_changed_here.len(),
        "tickets_not_attempted": report.tickets_not_attempted,
        "people_problems": report.people_problems.len(),
        "comments_written": report.comments_written,
        "attachments_written": report.attachments_written,
        "attachment_problems": report.attachment_problems.len(),
        "unreadable_lines": report.unreadable_lines,
        "people": report.people,
        "missing_people": report.missing_people.as_ref().map(|m| serde_json::json!({
            "people": m.people,
            "tickets": m.tickets,
            "replies": m.replies,
            "label": m.label,
        })),
        "gone_people": report.gone_people.as_ref().map(|m| m.people),
        "organizations_filled": report.profile.organizations,
        "phones_added": report.profile.phones,
        "profile_problems": report.profile_problems.len(),
        "changed_at_source": report.changed_at_source.len(),
    });
    let error = error.map(ToString::to_string);
    let written = i32::try_from(report.tickets_written).unwrap_or(i32::MAX);
    db(pool, opts.actor_context(), move |conn| {
        crate::repository::imports::update(
            conn,
            job,
            crate::models::ImportJobUpdate {
                status: Some(if error.is_some() { "failed" } else { "done" }.into()),
                completed_at: Some(Some(Utc::now())),
                summary: Some(Some(summary)),
                records_committed: Some(Some(written)),
                error_message: Some(error),
                ..Default::default()
            },
        )?;
        Ok(())
    })
    .await
}

/// Everything [`load`] writes, into `report` as it goes, so a run that
/// fails part way still says what it did.
async fn write_all(
    pool: &Pool,
    storage: Arc<dyn Storage>,
    bundle: &Bundle,
    opts: &LoadOptions,
    progress: &(dyn Fn(&LoadProgress) + Send + Sync),
    prepared: Prepared,
    report: &mut LoadReport,
) -> Result<(), LoadError> {
    let Prepared {
        manifest,
        found,
        groups,
        people_total,
        bad_people,
        missing,
        gone: _,
    } = prepared;
    let source = manifest.source.clone();
    let instance = manifest.source_scope.clone();
    let scope_parts = (opts.workspace_id, source.clone(), instance.clone());
    // How an absent person is named in a ticket's import note.
    let absent = Absent {
        label: missing.map_or_else(
            || Absence::of(manifest.people_source, bad_people > 0).label(&source),
            |m| m.label,
        ),
        gone: manifest
            .fill
            .iter()
            .flat_map(|f| f.gone.iter().cloned())
            .collect(),
        gone_label: Absence::GoneAtSource.label(&source),
    };

    // Settle numbering once, before the first batch, and reserve the
    // numbers being kept, so a ticket opened here meanwhile takes one after
    // them rather than one the import has still to write.
    let keep_numbers = {
        let (ws, s, i) = scope_parts.clone();
        let (policy, numbers_ok, top) = (opts.numbers, found.numbers_ok, found.top);
        db(pool, opts.actor_context(), move |conn| {
            let scope = Scope {
                workspace_id: ws,
                source: &s,
                instance: &i,
            };
            let keep = number_decision(conn, scope, policy, numbers_ok)?.0;
            if let (true, Some(top)) = (keep, top) {
                crate::repository::tickets::reserve_numbers(conn, top)?;
            }
            Ok(keep)
        })
        .await?
    };

    // People first: everyone the tickets mention, in batches, each under
    // the source's lock, with the placeholder author their unattachable
    // replies go to.
    report.unreadable_lines += bad_people;
    let mut prog = LoadProgress {
        people_total,
        tickets_total: found.ids.len() as u64,
        ..Default::default()
    };
    let placeholder: Uuid = {
        let (ws, s, i) = scope_parts.clone();
        let o = opts.clone();
        db(pool, opts.actor_context(), move |conn| {
            let popts = people_options(&o, ws, &s, &i);
            idmap::lock_source(conn, popts.scope)?;
            Ok(people::placeholder(conn, &popts)?)
        })
        .await?
    };
    let mut resolved: HashMap<String, Resolved> = HashMap::new();
    for chunk in group_batches(&groups, opts.people_batch_size.max(1)) {
        if opts.cancel.is_cancelled() {
            return Err(LoadError::Cancelled);
        }
        let (ws, s, i) = scope_parts.clone();
        let o = opts.clone();
        let chunk = chunk.to_vec();
        let done = db(pool, opts.actor_context(), move |conn| {
            let popts = people_options(&o, ws, &s, &i);
            idmap::lock_source(conn, popts.scope)?;
            Ok(people::apply(conn, &chunk, &popts, placeholder)?)
        })
        .await?;
        prog.people_done += done.resolved.len() as u64;
        resolved.extend(done.resolved);
        report.people_problems.extend(done.problems);
        report.profile_problems.extend(done.profile_problems);
        report.changed_at_source.extend(done.changed_at_source);
        report.profile.add(&done.profile);
        progress(&prog);
    }
    for r in resolved.values() {
        *report.people.entry(r.outcome.label()).or_default() += 1;
    }
    let people_map = Arc::new(resolved);
    let absent = Arc::new(absent);

    let batch_size = opts.batch_size.max(1);
    let mut lines = bundle.tickets()?;
    loop {
        if opts.cancel.is_cancelled() {
            return Err(LoadError::Cancelled);
        }
        let mut batch = Vec::with_capacity(batch_size);
        for t in lines.by_ref() {
            match t {
                Ok(t) => batch.push(t),
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                    report.unreadable_lines += 1
                }
                Err(e) => return Err(e.into()),
            }
            if batch.len() == batch_size {
                break;
            }
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
            &absent,
            batch.clone(),
        )
        .await?;
        report.tickets_written += written.tickets;
        report.tickets_updated += written.updated;
        report.tickets_skipped +=
            n - written.tickets - written.failed.len() as u64 - written.not_attempted;
        report.tickets_not_attempted += written.not_attempted;
        report.comments_written += written.comments;
        report.ticket_notes.extend(written.notes);
        report.tickets_failed.extend(written.failed);
        report.tickets_changed_here.extend(written.changed_here);
        report.numbers_moved.extend(written.numbers_moved);

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
        job_progress(pool, opts, report.tickets_written).await?;

        prog.tickets_done += n;
        prog.comments_written = report.comments_written;
        prog.attachments_written = report.attachments_written;
        progress(&prog);
    }
    Ok(())
}

/// `groups` in batches of about `size` people, never splitting a group.
fn group_batches(groups: &[people::AddressGroup], size: usize) -> Vec<&[people::AddressGroup]> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < groups.len() {
        let mut end = start;
        let mut n = 0;
        while end < groups.len() && (n == 0 || n + groups[end].members.len() <= size) {
            n += groups[end].members.len();
            end += 1;
        }
        out.push(&groups[start..end]);
        start = end;
    }
    out
}

fn people_options<'a>(
    o: &'a LoadOptions,
    workspace_id: i32,
    source: &'a str,
    instance: &'a str,
) -> PeopleOptions<'a> {
    PeopleOptions {
        scope: Scope {
            workspace_id,
            source,
            instance,
        },
        job_id: o.job_id,
        agent_map: &o.agent_map,
        create_agents: o.create_agents,
    }
}

#[derive(Default)]
struct BatchWritten {
    /// Tickets the run stopped before reaching.
    not_attempted: u64,
    tickets: u64,
    updated: u64,
    comments: u64,
    notes: Vec<String>,
    failed: Vec<String>,
    changed_here: Vec<String>,
    numbers_moved: Vec<String>,
}

/// What became of one ticket of a batch.
enum Outcome {
    Written {
        id: i32,
        comments: u64,
        moved_from: Option<i32>,
        number: i32,
    },
    Again {
        comments: u64,
        updated: bool,
        changed_here: bool,
    },
    Note(String),
    /// New replies added to the ticket this one was merged into.
    Merged {
        comments: u64,
        note: String,
    },
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
    absent: &Arc<Absent>,
    batch: Vec<bundle::Ticket>,
) -> Result<BatchWritten, LoadError> {
    let (s, i, o, people, absent) = (
        source.to_string(),
        instance.to_string(),
        opts.clone(),
        Arc::clone(people),
        Arc::clone(absent),
    );
    db(pool, opts.actor_context(), move |conn| {
        let scope = Scope {
            workspace_id: o.workspace_id,
            source: &s,
            instance: &i,
        };
        // One batch at a time per source and workspace, and the map read
        // under the lock, so an overlapping run skips what this one wrote.
        idmap::lock_source(conn, scope)?;
        let states = StateMap::load(conn)?;
        let cats = categories(conn)?;
        let mut tags = TagCache::load(conn)?;
        let ids: Vec<String> = batch.iter().map(|t| t.source_id.clone()).collect();
        let mapped = idmap::targets_of(conn, scope, Entity::Ticket, &ids)?;
        let mut imported = idmap::imported_tickets(conn, scope, &ids)?;

        let mut out = BatchWritten::default();
        let mut written_now: HashSet<String> = HashSet::new();
        for (n, t) in batch.iter().enumerate() {
            if o.cancel.is_cancelled() {
                out.not_attempted = (batch.len() - n) as u64;
                break;
            }
            let label = format!(
                "{} #{}",
                source_title(&s),
                t.number.map_or(t.source_id.clone(), |n| n.to_string())
            );
            let ctx = TicketCtx {
                scope,
                opts: &o,
                people: &people,
                placeholder,
                states: &states,
                absent: &absent,
            };
            // Each ticket in a savepoint: one that fails is rolled back alone,
            // with any tag it created, and the rest of the batch carries on.
            let tags_before = tags.clone();
            let r = conn.transaction::<_, LoadError, _>(|conn| {
                match (
                    imported.get(&t.source_id),
                    mapped.contains_key(&t.source_id),
                ) {
                    // Merged here: new replies carry on where the merge put
                    // the conversation, as inbound mail's do.
                    (Some(done), _) if done.merged => {
                        let Some(dest) = crate::repository::ticket_merge::merge_destination(
                            conn,
                            done.ticket_id,
                        )?
                        else {
                            return Ok(Outcome::Again {
                                comments: 0,
                                updated: false,
                                changed_here: false,
                            });
                        };
                        let comments = append_new_comments(
                            conn,
                            scope,
                            &o,
                            &people,
                            placeholder,
                            dest,
                            t,
                        )?;
                        if comments == 0 {
                            return Ok(Outcome::Again {
                                comments,
                                updated: false,
                                changed_here: false,
                            });
                        }
                        let number = crate::repository::tickets::get_ticket_by_id(conn, dest)?.number;
                        Ok(Outcome::Merged {
                            comments,
                            note: format!(
                                "{label}: merged here into #{number}, so its {comments} new replies were added there"
                            ),
                        })
                    }
                    (Some(done), _) => {
                        // A copy listed again in this bundle only adds
                        // replies; its fields were written moments ago.
                        let up = if written_now.contains(&t.source_id) {
                            UpToDate::Already
                        } else {
                            bring_up_to_date(conn, &ctx, done, t)?
                        };
                        // Deleted here between the batch read and now.
                        if up == UpToDate::Gone {
                            return Ok(Outcome::Note(format!(
                                "{label}: deleted here since it was imported, so not brought back"
                            )));
                        }
                        let (updated, changed_here) =
                            (up == UpToDate::Updated, up == UpToDate::ChangedHere);
                        let comments = append_new_comments(
                            conn,
                            scope,
                            &o,
                            &people,
                            placeholder,
                            done.ticket_id,
                            t,
                        )?;
                        Ok(Outcome::Again {
                            comments,
                            updated,
                            changed_here,
                        })
                    }
                    // Imported before and deleted here since: deleting it
                    // was someone's choice, so it stays deleted.
                    (None, true) => Ok(Outcome::Note(format!(
                        "{label}: deleted here since it was imported, so not brought back"
                    ))),
                    (None, false) => write_ticket(conn, &ctx, keep_numbers, &cats, &mut tags, t),
                }
            });
            match r {
                Ok(Outcome::Written {
                    id,
                    comments,
                    moved_from,
                    number,
                }) => {
                    out.tickets += 1;
                    out.comments += comments;
                    if let Some(wanted) = moved_from {
                        out.numbers_moved.push(format!(
                            "{label}: #{wanted} is taken here, so it is #{number}"
                        ));
                    }
                    written_now.insert(t.source_id.clone());
                    imported.insert(
                        t.source_id.clone(),
                        idmap::ImportedTicket {
                            source_id: t.source_id.clone(),
                            ticket_id: id,
                            merged: false,
                            untouched: true,
                            source_updated_at: Some(naive(t.updated_at)),
                        },
                    );
                }
                Ok(Outcome::Again {
                    comments,
                    updated,
                    changed_here,
                }) => {
                    out.comments += comments;
                    out.updated += u64::from(updated);
                    if changed_here {
                        out.changed_here.push(format!(
                            "{label}: changed here since it was imported, so its newer status \
                             and assignee there weren't applied"
                        ));
                    }
                }
                Ok(Outcome::Note(note)) => out.notes.push(note),
                Ok(Outcome::Merged { comments, note }) => {
                    out.comments += comments;
                    out.notes.push(note);
                }
                Err(e) => {
                    tags = tags_before;
                    out.failed.push(format!("{label}: {e}"));
                }
            }
        }
        Ok(out)
    })
    .await
}

/// What writing one ticket needs, alongside the connection.
struct TicketCtx<'a> {
    scope: Scope<'a>,
    opts: &'a LoadOptions,
    people: &'a HashMap<String, Resolved>,
    placeholder: Uuid,
    states: &'a StateMap,
    /// Why someone the ticket names isn't among `people`, in words.
    absent: &'a Absent,
}

/// A ticket imported before, during a cutover: when the source changed it
/// since the import last wrote it and nobody has changed it here, its
/// status, assignee and closing follow the source (the closing kept as
/// given, per the closing trigger's rule for a statement that sets it).
fn bring_up_to_date(
    conn: &mut DbConnection,
    ctx: &TicketCtx<'_>,
    done: &idmap::ImportedTicket,
    t: &bundle::Ticket,
) -> Result<UpToDate, LoadError> {
    let source_at = naive(t.updated_at);
    if done.source_updated_at.is_some_and(|at| source_at <= at) {
        return Ok(UpToDate::Already);
    }
    if !done.untouched {
        return Ok(UpToDate::ChangedHere);
    }
    // The batch read was without a lock: ask again holding the row, so a
    // change someone made since is seen, and none can land before ours.
    match idmap::lock_imported_ticket(conn, ctx.scope, &t.source_id)? {
        Some((_, true)) => {}
        Some((_, false)) => return Ok(UpToDate::ChangedHere),
        None => return Ok(UpToDate::Gone),
    }
    let current = crate::repository::tickets::get_ticket_by_id(conn, done.ticket_id)?;
    let (state_id, _) = ctx.states.state(t.status);
    let assignee = t
        .assignee
        .as_ref()
        .and_then(|a| ctx.people.get(a))
        .filter(|r| r.staff)
        .map(|r| r.uuid);
    let mut change = crate::models::TicketUpdate::default();
    if current.workflow_state_id != state_id {
        change.workflow_state_id = Some(state_id);
        change.closed_at = Some(
            t.status
                .is_finished()
                .then(|| naive(t.solved_at.unwrap_or(t.updated_at))),
        );
    }
    if current.assignee_uuid != assignee {
        change.assignee_uuid = Some(assignee);
    }
    let changed = change.workflow_state_id.is_some() || change.assignee_uuid.is_some();
    // The mark is the updated_at the import's own write left, so a change
    // anyone makes after it reads as later.
    let written_at = if changed {
        Some(
            crate::repository::tickets::update_ticket_partial(conn, done.ticket_id, change, None)
                .map_err(diesel::result::Error::from)?
                .updated_at,
        )
    } else {
        None
    };
    idmap::touch_ticket(conn, ctx.scope, &t.source_id, source_at, written_at)?;
    Ok(if changed {
        UpToDate::Updated
    } else {
        UpToDate::Already
    })
}

/// Where a ticket imported before stands after [`bring_up_to_date`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpToDate {
    /// Its status, assignee or closing were brought up to date.
    Updated,
    /// Nothing to bring: the source hasn't changed it, or it already
    /// matches.
    Already,
    /// Changed here since the import wrote it: left alone.
    ChangedHere,
    /// Deleted here since the batch read it.
    Gone,
}

#[derive(Clone)]
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

fn write_ticket(
    conn: &mut DbConnection,
    ctx: &TicketCtx<'_>,
    keep_numbers: bool,
    cats: &HashMap<String, (i32, String)>,
    tags: &mut TagCache,
    t: &bundle::Ticket,
) -> Result<Outcome, LoadError> {
    let TicketCtx {
        scope,
        opts,
        people,
        placeholder,
        states,
        absent,
    } = *ctx;
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
    // here or not: that is when the requester was first answered. The
    // opening message is the request, whoever wrote it (an agent logging a
    // call), so it never counts.
    let first_response = t
        .comments
        .iter()
        .skip(1)
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
    // A kept number already taken here (the source reused it, or another
    // ticket holds it) falls back to the next one, and the report says so.
    let wanted = t
        .number
        .filter(|_| keep_numbers)
        .and_then(|n| i32::try_from(n).ok());
    let number = match wanted {
        Some(n) if !crate::repository::tickets::number_taken(conn, opts.workspace_id, n)? => {
            Some(n)
        }
        _ => None,
    };

    let ticket: Ticket = crate::repository::tickets::import_ticket(
        conn,
        opts.workspace_id,
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
            requester_uuid: Some(requester.map_or(placeholder, |r| r.uuid)),
            assignee_uuid,
            category_id: category.map(|(id, _)| *id),
            submitted_via: Some(HELPDESK_IMPORT.to_string()),
            created_by: Some(requester.map_or(placeholder, |r| r.uuid)),
            number,
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
        crate::repository::tags::set_tags_for_ticket(conn, ticket.id, &tag_ids, None)?;
    }

    // Only people who hold a seat here watch: a former agent, someone
    // suspended at the source or an account the import couldn't attach
    // hears nothing of the ticket.
    for w in &t.watchers {
        if let Some(r) = people.get(w) {
            if r.seated && Some(r.uuid) != ticket.requester_uuid {
                crate::repository::ticket_watchers::add_watcher(conn, ticket.id, r.uuid, false)?;
            }
        }
    }

    // One internal note with what Nosdesk has no column for, dated with the
    // ticket so it sits at the top of the thread.
    let note = import_note(scope.source, t, requester, assignee, absent);
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
    Ok(Outcome::Written {
        id: ticket.id,
        comments,
        moved_from: wanted.filter(|_| number.is_none()),
        number: ticket.number,
    })
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
        // Kept only when the reply is credited to the placeholder in their
        // stead; otherwise the account says who they are.
        email: resolved
            .filter(|r| r.withheld())
            .and_then(|r| r.source_email.as_deref()),
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
    email: Option<&'a str>,
    staff: bool,
}

/// A source person by name and address, for a note.
fn who(r: &Resolved) -> String {
    match &r.source_email {
        Some(e) => format!("{} <{e}>", r.source_name),
        None => r.source_name.clone(),
    }
}

fn import_note(
    source: &str,
    t: &bundle::Ticket,
    requester: Option<&Resolved>,
    assignee: Option<&Resolved>,
    absent: &Absent,
) -> String {
    let title = source_title(source);
    let mut lines = vec![match t.number {
        Some(n) => format!("Imported from {title} #{n}."),
        None => format!("Imported from {title}."),
    }];
    lines.push(format!("Status there: {}.", t.status_label));
    // Someone the bundle doesn't hold is named by source id only: it is all
    // the bundle has, and enough to look them up there. Someone deleted
    // there is named by name only.
    match (&t.requester, requester) {
        (Some(id), None) => lines.push(format!(
            "Requested there by {title} user {id} ({}); credited to the placeholder author.",
            absent.of(id)
        )),
        (_, Some(r)) if r.deleted() => lines.push(format!(
            "Requested there by {} (deleted at {title}); credited to the placeholder author.",
            r.source_name
        )),
        (_, Some(r)) if r.withheld() => lines.push(format!(
            "Requested there by {}; credited to the placeholder author.",
            who(r)
        )),
        _ => {}
    }
    match (&t.assignee, assignee) {
        (Some(id), None) => lines.push(format!(
            "It was assigned to {title} user {id} ({}), so it is unassigned.",
            absent.of(id)
        )),
        (_, Some(a)) if a.deleted() => lines.push(format!(
            "It was assigned to {} (deleted at {title}), so it is unassigned.",
            a.source_name
        )),
        (_, Some(a)) if a.withheld() => lines.push(format!(
            "It was assigned to {}, who has no seat here, so it is unassigned.",
            who(a)
        )),
        (_, Some(a)) if !a.staff => lines.push(format!(
            "It was assigned to {}, who has no staff seat here, so it is unassigned.",
            a.source_name
        )),
        _ => {}
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
                    "author_email": source_author.as_ref().and_then(|a| a.email),
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
    let ws_id = opts.workspace_id;
    let (comments, attached, tickets) = db(pool, opts.actor_context(), move |conn| {
        let scope = Scope {
            workspace_id: ws_id,
            source: &s,
            instance: &i,
        };
        let comments = idmap::targets_of(conn, scope, Entity::Comment, &comment_ids)?;
        let attached = idmap::targets_of(conn, scope, Entity::Attachment, &file_ids)?;
        let ids: Vec<i32> = comments.values().filter_map(|v| v.parse().ok()).collect();
        let ticket_of: HashMap<String, i32> =
            crate::repository::comments::tickets_of(conn, ws_id, &ids)?
                .into_iter()
                .map(|(id, t)| (id.to_string(), t))
                .collect();
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
                // Between files only, so a file stored is always attached;
                // the run stops before its next batch.
                if opts.cancel.is_cancelled() {
                    return Ok(out);
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
                let (bytes, mime) = match read_checked(bundle, &a.source_id, &a.file_name) {
                    Ok(read) => read,
                    Err(why) => {
                        out.problems.push(format!("{label}: {why}"));
                        continue;
                    }
                };
                let checksum = hex::encode(Sha256::digest(&bytes));
                let name =
                    crate::utils::file_validation::FileValidator::sanitize_filename(&a.file_name)
                        .unwrap_or_else(|_| "attachment".to_string());
                let stored = match scoped
                    .store_file(&bytes, &name, &mime, &format!("tickets/{ticket_id}"))
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
                let size = bytes.len() as i64;
                let url = stored.url.clone();
                let row = db(pool, opts.actor_context(), move |conn| {
                    let scope = Scope {
                        workspace_id: ws_id,
                        source: &s,
                        instance: &i,
                    };
                    idmap::lock_source(conn, scope)?;
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

/// An attachment's bytes from the bundle, with the type they are, checked
/// as an upload is: no larger than the instance takes (`MAX_FILE_SIZE_MB`),
/// and nothing a staff upload would refuse (a blocked extension, an SVG,
/// an executable). Read once, from a regular file inside the bundle.
fn read_checked(
    bundle: &Bundle,
    source_id: &str,
    file_name: &str,
) -> Result<(Vec<u8>, String), String> {
    use crate::utils::file_validation::{get_max_file_size, FileValidator};
    use std::io::Read;
    let (file, len) = bundle
        .open_file(source_id)
        .map_err(|e| format!("not readable in the bundle ({e})"))?;
    let max = get_max_file_size() as u64;
    if len > max {
        return Err(format!(
            "larger than the {} MB this server takes",
            max / 1024 / 1024
        ));
    }
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("not readable in the bundle ({e})"))?;
    if bytes.len() as u64 > max {
        return Err(format!(
            "larger than the {} MB this server takes",
            max / 1024 / 1024
        ));
    }
    let mime = FileValidator::validate_file(&bytes, Some(file_name)).map_err(|e| e.to_string())?;
    Ok((bytes, mime))
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

    fn missing(absence: Absence) -> MissingPeople {
        MissingPeople {
            people: 2,
            tickets: 1,
            replies: 3,
            absence,
            label: absence.label("zendesk"),
        }
    }

    #[test]
    fn absent_people_are_labelled_by_where_the_people_came_from() {
        use bundle::PeopleSource;
        assert_eq!(
            Absence::of(Some(PeopleSource::Export), false),
            Absence::NotInExport
        );
        assert_eq!(
            Absence::of(Some(PeopleSource::Api), false),
            Absence::GoneAtSource
        );
        assert_eq!(Absence::of(None, false), Absence::NotInBundle);
        // An unreadable people line may be the absent person: not gone.
        assert_eq!(
            Absence::of(Some(PeopleSource::Api), true),
            Absence::Unreadable
        );
        assert_eq!(
            Absence::of(Some(PeopleSource::Export), true),
            Absence::NotInExport
        );
        assert_eq!(Absence::NotInExport.label("zendesk"), "not in the export");
        assert_eq!(Absence::GoneAtSource.label("zendesk"), "gone at Zendesk");
        assert_eq!(Absence::NotInBundle.label("zendesk"), "not in the bundle");
        assert_eq!(
            Absence::Unreadable.label("zendesk"),
            "unreadable in the bundle"
        );
    }

    #[test]
    fn only_people_the_source_confirms_gone_need_no_flag() {
        let why = missing(Absence::NotInExport)
            .refusal(false)
            .expect("refused");
        assert!(
            why.contains(
                "2 people the bundle doesn't hold (not in the export), on 1 ticket and 3 replies"
            ),
            "{why}"
        );
        assert!(why.contains("--allow-missing-people"), "{why}");
        assert!(why.contains("users export"), "{why}");
        assert!(why.contains("--refetch"), "{why}");
        assert_eq!(missing(Absence::NotInExport).refusal(true), None);

        let why = missing(Absence::NotInBundle)
            .refusal(false)
            .expect("refused");
        assert!(why.contains("--refetch"), "{why}");
        assert!(
            why.contains("a credential") && why.contains("users export"),
            "{why}"
        );
        assert_eq!(missing(Absence::NotInBundle).refusal(true), None);

        let why = missing(Absence::Unreadable)
            .refusal(false)
            .expect("refused");
        assert!(why.contains("(unreadable in the bundle)"), "{why}");
        assert!(why.contains("--refetch"), "{why}");
        assert_eq!(missing(Absence::Unreadable).refusal(true), None);

        assert_eq!(missing(Absence::GoneAtSource).refusal(false), None);
        let nobody = MissingPeople {
            people: 0,
            ..missing(Absence::NotInExport)
        };
        assert_eq!(nobody.refusal(false), None);
    }

    #[test]
    fn the_note_names_an_absent_requester_and_assignee_by_id_only() {
        let t = bundle::Ticket {
            source_id: "7".into(),
            number: Some(7),
            subject: "s".into(),
            status: StatusKind::Open,
            status_label: "open".into(),
            priority: None,
            ticket_type: None,
            tags: vec![],
            requester: Some("901".into()),
            assignee: Some("902".into()),
            watchers: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            solved_at: None,
            organization_id: None,
            fields: BTreeMap::new(),
            comments: vec![],
        };
        let absent = Absent {
            label: "not in the export".into(),
            gone: HashSet::from(["902".to_string()]),
            gone_label: "gone at Zendesk".into(),
        };
        let note = import_note("zendesk", &t, None, None, &absent);
        assert!(
            note.contains(
                "Requested there by Zendesk user 901 (not in the export); credited to the \
                 placeholder author."
            ),
            "{note}"
        );
        assert!(
            note.contains(
                "It was assigned to Zendesk user 902 (gone at Zendesk), so it is unassigned."
            ),
            "{note}"
        );
        let nobody = bundle::Ticket {
            requester: None,
            assignee: None,
            ..t
        };
        let note = import_note("zendesk", &nobody, None, None, &absent);
        assert!(!note.contains("user"), "{note}");
    }
}
