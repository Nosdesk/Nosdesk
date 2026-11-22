//! An asset's timeline: what happened to the equipment, newest first.
//!
//! Each source is read with its own query over the asset ids asked for
//! (the asset, its predecessors, or every asset an export covers), ordered
//! the way the timeline orders it and cut at the cursor, and the sources are
//! merged here by [`EntryKey`]:
//!
//! - `status`: lifecycle events, each carrying its disposal when it disposed
//!   of the asset;
//! - `record`: asset records (staff only);
//! - `replacement`: a record that names a replacement asset, on both assets
//!   (staff only);
//! - `loan`: a loan's start and, once back, its return;
//! - `ticket`: tickets linked to the asset that the viewer can see;
//! - `marker`: the purchase date and when the asset was added.
//!
//! The order is total: the day (a timestamp's day in the workspace's
//! timezone), timed entries before date-only ones of the same day, the time,
//! when the entry was written, a fixed source rank, the id and the asset. So
//! a cursor naming the last entry of a page always continues where it left
//! off, ties or not.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use diesel::dsl::sql;
use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::sql_types::{Bool, Date, Integer, Timestamptz};
use diesel::QueryResult;

use crate::db::DbConnection;
use crate::models::{
    AssetDisposal, AssetLifecycleEvent, AssetLoan, AssetRecord, WorkflowStateCategory,
};
use crate::repository::asset_records::workspace_timezone;
use crate::repository::ticket_visibility::{visible_tickets_query, VisibilityContext};
use crate::schema::{
    asset_disposals, asset_lifecycle_events, asset_loans, asset_records, assets, ticket_assets,
    tickets, workflow_states,
};
use crate::utils::locale::day_start;

/// How far back a chain of replacements is followed.
pub const PREDECESSOR_HOPS: usize = 10;

/// Where a timeline entry comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Status,
    Record,
    Replacement,
    Loan,
    Ticket,
    Marker,
}

impl Source {
    pub const ALL: [Source; 6] = [
        Source::Status,
        Source::Record,
        Source::Replacement,
        Source::Loan,
        Source::Ticket,
        Source::Marker,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Source::Status => "status",
            Source::Record => "record",
            Source::Replacement => "replacement",
            Source::Loan => "loan",
            Source::Ticket => "ticket",
            Source::Marker => "marker",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == raw)
    }

    /// Sources only staff see.
    pub fn staff_only(self) -> bool {
        matches!(self, Source::Record | Source::Replacement)
    }
}

/// Which of two entries sorts first when everything before it ties: the
/// higher rank, newest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rank {
    Marker = 0,
    Ticket = 1,
    LoanOut = 2,
    LoanReturned = 3,
    Replacement = 4,
    Record = 5,
    Status = 6,
}

impl Rank {
    fn from_index(i: u8) -> Option<Self> {
        Some(match i {
            0 => Rank::Marker,
            1 => Rank::Ticket,
            2 => Rank::LoanOut,
            3 => Rank::LoanReturned,
            4 => Rank::Replacement,
            5 => Rank::Record,
            6 => Rank::Status,
            _ => return None,
        })
    }
}

/// An entry's place in the timeline. Entries sort newest first, which is
/// this key descending; fields compare in declaration order, and a date-only
/// entry (`at` of `None`) sorts below the timed entries of its day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryKey {
    pub day: NaiveDate,
    pub at: Option<DateTime<Utc>>,
    /// When the entry was written down, which for most sources is when it
    /// happened.
    pub recorded_at: DateTime<Utc>,
    pub rank: Rank,
    pub id: i32,
    /// The asset whose timeline holds the entry: a ticket linked to two
    /// assets of a chain is an entry on each.
    pub asset_id: i32,
}

impl EntryKey {
    /// The key as an opaque cursor.
    pub fn encode(&self) -> String {
        let at = self
            .at
            .map(|at| at.timestamp_micros().to_string())
            .unwrap_or_default();
        format!(
            "{}~{at}~{}~{}~{}~{}",
            self.day,
            self.recorded_at.timestamp_micros(),
            self.rank as u8,
            self.id,
            self.asset_id
        )
    }

    /// A cursor [`Self::encode`] wrote, else `None`.
    pub fn decode(raw: &str) -> Option<Self> {
        let mut parts = raw.split('~');
        let day = parts.next()?.parse().ok()?;
        let at = match parts.next()? {
            "" => None,
            micros => Some(DateTime::from_timestamp_micros(micros.parse().ok()?)?),
        };
        let recorded_at = DateTime::from_timestamp_micros(parts.next()?.parse().ok()?)?;
        let rank = Rank::from_index(parts.next()?.parse().ok()?)?;
        let id = parts.next()?.parse().ok()?;
        let asset_id = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            day,
            at,
            recorded_at,
            rank,
            id,
            asset_id,
        })
    }
}

/// A ticket as the timeline shows it.
#[derive(Debug, Clone)]
pub struct TimelineTicket {
    pub id: i32,
    pub number: i32,
    pub title: String,
    pub state: Option<TicketState>,
    pub created_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    /// When the ticket was linked to the asset.
    pub linked_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct TicketState {
    pub id: i32,
    pub name: String,
    pub category: WorkflowStateCategory,
}

/// The asset at the other end of a replacement.
#[derive(Debug, Clone)]
pub struct AssetRef {
    pub id: i32,
    pub name: String,
    pub asset_tag: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// The asset's purchase date.
    Purchased,
    /// When the asset was added to Nosdesk.
    Added,
}

impl Marker {
    pub fn as_str(self) -> &'static str {
        match self {
            Marker::Purchased => "purchased",
            Marker::Added => "added",
        }
    }
}

/// What an entry is.
#[derive(Debug, Clone)]
pub enum EntryData {
    Status {
        event: AssetLifecycleEvent,
        /// The disposal record, on the event that disposed of the asset.
        disposal: Option<AssetDisposal>,
        /// Records the transition opened. Empty for anyone but staff.
        record_ids: Vec<i32>,
    },
    Record(AssetRecord),
    Replacement {
        record: AssetRecord,
        /// True on the replacement's timeline ("Replaces CAM-014"), false on
        /// the replaced asset's ("Replaced by CAM-022").
        replaces: bool,
        /// The other asset; `None` once it is gone.
        other: Option<AssetRef>,
    },
    LoanOut(AssetLoan),
    LoanReturned(AssetLoan),
    Ticket(TimelineTicket),
    Marker(Marker),
}

impl EntryData {
    pub fn source(&self) -> Source {
        match self {
            EntryData::Status { .. } => Source::Status,
            EntryData::Record(_) => Source::Record,
            EntryData::Replacement { .. } => Source::Replacement,
            EntryData::LoanOut(_) | EntryData::LoanReturned(_) => Source::Loan,
            EntryData::Ticket(_) => Source::Ticket,
            EntryData::Marker(_) => Source::Marker,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub key: EntryKey,
    pub data: EntryData,
}

/// Who reads the timeline.
#[derive(Clone, Copy)]
pub struct Viewer {
    /// Staff see records and replacements, and every field of the rest.
    pub staff: bool,
    /// Which tickets they can see.
    pub tickets: VisibilityContext,
}

/// Which entries a timeline lists.
#[derive(Debug, Clone)]
pub struct TimelineFilter {
    pub sources: HashSet<Source>,
    /// Records (and replacements) of this type only.
    pub record_type_id: Option<i32>,
    /// Entries on or after this day.
    pub from: Option<NaiveDate>,
    /// Entries on or before this day.
    pub to: Option<NaiveDate>,
}

impl Default for TimelineFilter {
    fn default() -> Self {
        Self {
            sources: Source::ALL.into_iter().collect(),
            record_type_id: None,
            from: None,
            to: None,
        }
    }
}

/// A page of a timeline, and where the next one starts.
#[derive(Debug, Clone)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub next: Option<EntryKey>,
}

/// The assets `asset_id` replaced, and the ones they replaced, nearest
/// first: following records whose replacement is the asset, up to
/// [`PREDECESSOR_HOPS`] steps back, and stopping at an asset already seen,
/// so a chain that loops ends.
pub fn predecessors(conn: &mut DbConnection, asset_id: i32) -> QueryResult<Vec<i32>> {
    let mut seen: HashSet<i32> = HashSet::from([asset_id]);
    let mut chain = Vec::new();
    let mut frontier = vec![asset_id];
    for _ in 0..PREDECESSOR_HOPS {
        if frontier.is_empty() {
            break;
        }
        let found: Vec<i32> = asset_records::table
            .filter(asset_records::replacement_asset_id.eq_any(&frontier))
            .select(asset_records::asset_id)
            .distinct()
            .order(asset_records::asset_id)
            .load(conn)?;
        frontier = found.into_iter().filter(|id| seen.insert(*id)).collect();
        chain.extend(&frontier);
    }
    Ok(chain)
}

/// A page of the timeline of `asset_ids` as `viewer` sees it: the entries
/// after `after` (the cursor of the page before), at most `limit` of them,
/// or every one when `limit` is `None`.
pub fn page(
    conn: &mut DbConnection,
    asset_ids: &[i32],
    viewer: &Viewer,
    filter: &TimelineFilter,
    after: Option<EntryKey>,
    limit: Option<i64>,
) -> QueryResult<Page> {
    if asset_ids.is_empty() {
        return Ok(Page {
            entries: Vec::new(),
            next: None,
        });
    }
    let tz = workspace_timezone(conn)?;
    let read = Read {
        asset_ids,
        filter,
        after: after.as_ref(),
        // One past the page tells whether a source has more.
        fetch: limit.map(|l| l.max(1) + 1),
        tz,
    };
    let wants = |s: Source| filter.sources.contains(&s) && (viewer.staff || !s.staff_only());

    let mut entries: Vec<Entry> = Vec::new();
    let mut more = false;
    let mut add = |(found, has_more): (Vec<Entry>, bool)| {
        entries.extend(found);
        more |= has_more;
    };
    if wants(Source::Status) {
        add(read.status(conn)?);
    }
    if wants(Source::Record) {
        add(read.records(conn)?);
    }
    if wants(Source::Replacement) {
        add(read.replacements(conn, false)?);
        add(read.replacements(conn, true)?);
    }
    if wants(Source::Loan) {
        add(read.loans_out(conn)?);
        add(read.loans_returned(conn)?);
    }
    if wants(Source::Ticket) {
        add(read.tickets(conn, &viewer.tickets)?);
    }
    if wants(Source::Marker) {
        add(read.markers(conn)?);
    }

    // A source's query admits the cursor's own ties; drop them here.
    if let Some(after) = after {
        entries.retain(|e| e.key < after);
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.key));
    let mut next = None;
    if let Some(limit) = limit {
        let limit = limit.max(1) as usize;
        if entries.len() > limit {
            entries.truncate(limit);
            more = true;
        }
        if more {
            next = entries.last().map(|e| e.key);
        }
    }

    attach_status_details(conn, &mut entries, viewer)?;
    Ok(Page { entries, next })
}

/// Every record of `asset_ids`, of one type when given, by asset and then
/// newest work first, for the records export.
pub fn records_for_export(
    conn: &mut DbConnection,
    asset_ids: &[i32],
    record_type_id: Option<i32>,
) -> QueryResult<Vec<AssetRecord>> {
    let mut query = asset_records::table
        .filter(asset_records::asset_id.eq_any(asset_ids))
        .into_boxed();
    if let Some(type_id) = record_type_id {
        query = query.filter(asset_records::record_type_id.eq(type_id));
    }
    query
        .order((
            asset_records::asset_id.asc(),
            asset_records::occurred_on.desc(),
            asset_records::occurred_at.desc().nulls_last(),
            asset_records::id.desc(),
        ))
        .load(conn)
}

/// The tags of `asset_ids` that have one, for export cells naming another
/// asset.
pub fn asset_tags(conn: &mut DbConnection, asset_ids: &[i32]) -> QueryResult<HashMap<i32, String>> {
    if asset_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(i32, Option<String>)> = assets::table
        .filter(assets::id.eq_any(asset_ids))
        .select((assets::id, assets::asset_tag))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, tag)| tag.map(|t| (id, t)))
        .collect())
}

/// Ticket numbers by id.
pub fn ticket_numbers(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<HashMap<i32, i32>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(i32, i32)> = tickets::table
        .filter(tickets::id.eq_any(ids))
        .select((tickets::id, tickets::number))
        .load(conn)?;
    Ok(rows.into_iter().collect())
}

/// A filter condition usable on any of the timeline's queries.
type Cond<QS> = Box<dyn BoxableExpression<QS, Pg, SqlType = Bool>>;

/// A value a condition compares a column with.
#[derive(Debug, Clone, Copy)]
enum Val {
    Day(NaiveDate),
    At(DateTime<Utc>),
    Id(i32),
}

/// `column op value`, the value bound. `column` and `op` are fixed text.
fn cmp<QS: 'static>(column: &'static str, op: &'static str, value: Val) -> Cond<QS> {
    let head = format!("{column} {op} ");
    match value {
        Val::Day(day) => Box::new(sql::<Bool>(&head).bind::<Date, _>(day)),
        Val::At(at) => Box::new(sql::<Bool>(&head).bind::<Timestamptz, _>(at)),
        Val::Id(id) => Box::new(sql::<Bool>(&head).bind::<Integer, _>(id)),
    }
}

fn literal<QS: 'static>(value: bool) -> Cond<QS> {
    Box::new(sql::<Bool>(if value { "TRUE" } else { "FALSE" }))
}

/// The row of `terms` columns comes before the row of their values, or
/// equals it when `or_equal`: the row comparison spelled out, as
/// `a < x OR (a = x AND (b < y OR ...))`.
fn row_below<QS: 'static>(terms: &[(&'static str, Val)], or_equal: bool) -> Cond<QS> {
    terms
        .iter()
        .rev()
        .fold(literal(or_equal), |rest, &(column, value)| {
            let next: Cond<QS> =
                Box::new(cmp(column, "<", value).or(cmp(column, "=", value).and(rest)));
            next
        })
}

/// Entries of a source of rank `rank` whose leading key fields are `terms`
/// come after cursor `c` when those fields come before the cursor's, or tie
/// with them and the rank, id and asset do. The asset is left to the caller
/// (a tie on everything else keeps the row) when `asset_ties` is set.
fn below_cursor<QS: 'static>(
    mut terms: Vec<(&'static str, Val)>,
    id: &'static str,
    rank: Rank,
    c: &EntryKey,
    asset_ties: bool,
) -> Cond<QS> {
    match rank.cmp(&c.rank) {
        std::cmp::Ordering::Less => row_below(&terms, true),
        std::cmp::Ordering::Greater => row_below(&terms, false),
        std::cmp::Ordering::Equal => {
            terms.push((id, Val::Id(c.id)));
            row_below(&terms, asset_ties)
        }
    }
}

/// The condition for a timestamped source (its day is the timestamp's day
/// in `tz`) to come after cursor `c`.
fn timed_after<QS: 'static>(
    at: &'static str,
    recorded_at: &'static str,
    id: &'static str,
    rank: Rank,
    c: &EntryKey,
    tz: Tz,
    asset_ties: bool,
) -> Cond<QS> {
    let start = day_start(c.day, tz);
    let earlier_day = cmp(at, "<", Val::At(start));
    // Timed entries sort above the date-only ones of their day.
    let Some(cursor_at) = c.at else {
        return earlier_day;
    };
    let before_next_day = match c.day.succ_opt() {
        Some(next) => cmp(at, "<", Val::At(day_start(next, tz))),
        None => literal(true),
    };
    let same_day_later = below_cursor(
        vec![
            (at, Val::At(cursor_at)),
            (recorded_at, Val::At(c.recorded_at)),
        ],
        id,
        rank,
        c,
        asset_ties,
    );
    Box::new(
        earlier_day.or(cmp(at, ">=", Val::At(start))
            .and(before_next_day)
            .and(same_day_later)),
    )
}

/// The condition for a record-shaped source (its day is `occurred_on`, its
/// time `occurred_at` when known) to come after cursor `c`.
fn dated_after<QS: 'static>(rank: Rank, c: &EntryKey) -> Cond<QS> {
    const ON: &str = "asset_records.occurred_on";
    const AT: &str = "asset_records.occurred_at";
    const REC: &str = "asset_records.recorded_at";
    const ID: &str = "asset_records.id";
    let earlier_day = cmp(ON, "<", Val::Day(c.day));
    let same_day: Cond<QS> = match c.at {
        // Below a date-only cursor: the date-only entries that sort below it.
        None => Box::new(sql::<Bool>(&format!("{AT} IS NULL")).and(below_cursor(
            vec![(REC, Val::At(c.recorded_at))],
            ID,
            rank,
            c,
            false,
        ))),
        // Below a timed cursor: every date-only entry, and the timed ones
        // below it.
        Some(cursor_at) => Box::new(sql::<Bool>(&format!("{AT} IS NULL")).or(
            sql::<Bool>(&format!("{AT} IS NOT NULL")).and(below_cursor(
                vec![(AT, Val::At(cursor_at)), (REC, Val::At(c.recorded_at))],
                ID,
                rank,
                c,
                false,
            )),
        )),
    };
    Box::new(earlier_day.or(cmp(ON, "=", Val::Day(c.day)).and(same_day)))
}

/// One read of the timeline: the assets, the filter, the cursor and how many
/// rows each source fetches.
struct Read<'a> {
    asset_ids: &'a [i32],
    filter: &'a TimelineFilter,
    after: Option<&'a EntryKey>,
    fetch: Option<i64>,
    tz: Tz,
}

impl Read<'_> {
    fn day_of(&self, at: DateTime<Utc>) -> NaiveDate {
        at.with_timezone(&self.tz).date_naive()
    }

    /// Whether a query that returned `n` rows left some behind.
    fn more(&self, n: usize) -> bool {
        self.fetch.is_some_and(|f| n as i64 >= f)
    }

    /// The from/to window on a timestamp column.
    fn window<QS: 'static>(&self, at: &'static str) -> Cond<QS> {
        let mut cond = literal(true);
        if let Some(from) = self.filter.from {
            cond = Box::new(cond.and(cmp(at, ">=", Val::At(day_start(from, self.tz)))));
        }
        if let Some(next) = self.filter.to.and_then(|to| to.succ_opt()) {
            cond = Box::new(cond.and(cmp(at, "<", Val::At(day_start(next, self.tz)))));
        }
        cond
    }

    fn status(&self, conn: &mut DbConnection) -> QueryResult<(Vec<Entry>, bool)> {
        const AT: &str = "asset_lifecycle_events.occurred_at";
        const ID: &str = "asset_lifecycle_events.id";
        let mut query = asset_lifecycle_events::table
            .filter(asset_lifecycle_events::asset_id.eq_any(self.asset_ids))
            .filter(self.window(AT))
            .into_boxed();
        if let Some(c) = self.after {
            query = query.filter(timed_after(AT, AT, ID, Rank::Status, c, self.tz, false));
        }
        query = query.order((
            asset_lifecycle_events::occurred_at.desc(),
            asset_lifecycle_events::id.desc(),
        ));
        if let Some(fetch) = self.fetch {
            query = query.limit(fetch);
        }
        let rows: Vec<AssetLifecycleEvent> = query.load(conn)?;
        let more = self.more(rows.len());
        let entries = rows
            .into_iter()
            .map(|event| Entry {
                key: EntryKey {
                    day: self.day_of(event.occurred_at),
                    at: Some(event.occurred_at),
                    recorded_at: event.occurred_at,
                    rank: Rank::Status,
                    id: event.id,
                    asset_id: event.asset_id,
                },
                data: EntryData::Status {
                    event,
                    disposal: None,
                    record_ids: Vec::new(),
                },
            })
            .collect();
        Ok((entries, more))
    }

    /// Records, or with `rank` of `Replacement` the records naming a
    /// replacement; `on_replacement` reads those on the replacement's side.
    fn record_rows(
        &self,
        conn: &mut DbConnection,
        rank: Rank,
        on_replacement: bool,
    ) -> QueryResult<Vec<AssetRecord>> {
        let mut query = asset_records::table.into_boxed();
        query = if on_replacement {
            query.filter(asset_records::replacement_asset_id.eq_any(self.asset_ids))
        } else {
            query.filter(asset_records::asset_id.eq_any(self.asset_ids))
        };
        if rank == Rank::Replacement && !on_replacement {
            query = query.filter(asset_records::replacement_asset_id.is_not_null());
        }
        if let Some(type_id) = self.filter.record_type_id {
            query = query.filter(asset_records::record_type_id.eq(type_id));
        }
        if let Some(from) = self.filter.from {
            query = query.filter(asset_records::occurred_on.ge(from));
        }
        if let Some(to) = self.filter.to {
            query = query.filter(asset_records::occurred_on.le(to));
        }
        if let Some(c) = self.after {
            query = query.filter(dated_after(rank, c));
        }
        query = query.order((
            asset_records::occurred_on.desc(),
            asset_records::occurred_at.desc().nulls_last(),
            asset_records::recorded_at.desc(),
            asset_records::id.desc(),
        ));
        if let Some(fetch) = self.fetch {
            query = query.limit(fetch);
        }
        query.load(conn)
    }

    fn record_key(record: &AssetRecord, rank: Rank, asset_id: i32) -> EntryKey {
        EntryKey {
            day: record.occurred_on,
            at: record.occurred_at,
            recorded_at: record.recorded_at,
            rank,
            id: record.id,
            asset_id,
        }
    }

    fn records(&self, conn: &mut DbConnection) -> QueryResult<(Vec<Entry>, bool)> {
        let rows = self.record_rows(conn, Rank::Record, false)?;
        let more = self.more(rows.len());
        let entries = rows
            .into_iter()
            .map(|record| Entry {
                key: Self::record_key(&record, Rank::Record, record.asset_id),
                data: EntryData::Record(record),
            })
            .collect();
        Ok((entries, more))
    }

    /// "Replaced by" entries on the replaced assets or, with `replaces`,
    /// "Replaces" entries on the replacements.
    fn replacements(
        &self,
        conn: &mut DbConnection,
        replaces: bool,
    ) -> QueryResult<(Vec<Entry>, bool)> {
        let rows = self.record_rows(conn, Rank::Replacement, replaces)?;
        let more = self.more(rows.len());
        let others: Vec<i32> = rows
            .iter()
            .filter_map(|r| {
                if replaces {
                    Some(r.asset_id)
                } else {
                    r.replacement_asset_id
                }
            })
            .collect();
        let named: HashMap<i32, AssetRef> = if others.is_empty() {
            HashMap::new()
        } else {
            assets::table
                .filter(assets::id.eq_any(&others))
                .select((assets::id, assets::name, assets::asset_tag))
                .load::<(i32, String, Option<String>)>(conn)?
                .into_iter()
                .map(|(id, name, asset_tag)| {
                    (
                        id,
                        AssetRef {
                            id,
                            name,
                            asset_tag,
                        },
                    )
                })
                .collect()
        };
        let entries = rows
            .into_iter()
            .filter_map(|record| {
                let (holder, other) = if replaces {
                    (record.replacement_asset_id?, record.asset_id)
                } else {
                    (record.asset_id, record.replacement_asset_id?)
                };
                Some(Entry {
                    key: Self::record_key(&record, Rank::Replacement, holder),
                    data: EntryData::Replacement {
                        other: named.get(&other).cloned(),
                        record,
                        replaces,
                    },
                })
            })
            .collect();
        Ok((entries, more))
    }

    fn loans_out(&self, conn: &mut DbConnection) -> QueryResult<(Vec<Entry>, bool)> {
        const AT: &str = "asset_loans.loaned_at";
        const REC: &str = "asset_loans.created_at";
        const ID: &str = "asset_loans.id";
        let mut query = asset_loans::table
            .filter(asset_loans::asset_id.eq_any(self.asset_ids))
            .filter(self.window(AT))
            .into_boxed();
        if let Some(c) = self.after {
            query = query.filter(timed_after(AT, REC, ID, Rank::LoanOut, c, self.tz, false));
        }
        query = query.order((
            asset_loans::loaned_at.desc(),
            asset_loans::created_at.desc(),
            asset_loans::id.desc(),
        ));
        if let Some(fetch) = self.fetch {
            query = query.limit(fetch);
        }
        let rows: Vec<AssetLoan> = query.load(conn)?;
        let more = self.more(rows.len());
        let entries = rows
            .into_iter()
            .map(|loan| Entry {
                key: EntryKey {
                    day: self.day_of(loan.loaned_at),
                    at: Some(loan.loaned_at),
                    recorded_at: loan.created_at,
                    rank: Rank::LoanOut,
                    id: loan.id,
                    asset_id: loan.asset_id,
                },
                data: EntryData::LoanOut(loan),
            })
            .collect();
        Ok((entries, more))
    }

    fn loans_returned(&self, conn: &mut DbConnection) -> QueryResult<(Vec<Entry>, bool)> {
        const AT: &str = "asset_loans.returned_at";
        const ID: &str = "asset_loans.id";
        let mut query = asset_loans::table
            .filter(asset_loans::asset_id.eq_any(self.asset_ids))
            .filter(asset_loans::returned_at.is_not_null())
            .filter(self.window(AT))
            .into_boxed();
        if let Some(c) = self.after {
            query = query.filter(timed_after(
                AT,
                AT,
                ID,
                Rank::LoanReturned,
                c,
                self.tz,
                false,
            ));
        }
        query = query.order((asset_loans::returned_at.desc(), asset_loans::id.desc()));
        if let Some(fetch) = self.fetch {
            query = query.limit(fetch);
        }
        let rows: Vec<AssetLoan> = query.load(conn)?;
        let more = self.more(rows.len());
        let entries = rows
            .into_iter()
            .filter_map(|loan| {
                let returned_at = loan.returned_at?;
                Some(Entry {
                    key: EntryKey {
                        day: self.day_of(returned_at),
                        at: Some(returned_at),
                        recorded_at: returned_at,
                        rank: Rank::LoanReturned,
                        id: loan.id,
                        asset_id: loan.asset_id,
                    },
                    data: EntryData::LoanReturned(loan),
                })
            })
            .collect();
        Ok((entries, more))
    }

    /// Tickets linked to the assets that `visibility` can see: one entry per
    /// ticket and linked asset, at the ticket's creation.
    fn tickets(
        &self,
        conn: &mut DbConnection,
        visibility: &VisibilityContext,
    ) -> QueryResult<(Vec<Entry>, bool)> {
        const AT: &str = "tickets.created_at";
        const ID: &str = "tickets.id";
        let linked = ticket_assets::table
            .filter(ticket_assets::asset_id.eq_any(self.asset_ids))
            .select(ticket_assets::ticket_id);
        let mut query = visible_tickets_query(visibility)
            .filter(tickets::id.eq_any(linked))
            .filter(self.window(AT));
        if let Some(c) = self.after {
            // A ticket linked to two of the assets ties on everything but the
            // asset: keep the cursor's ticket and leave the asset to the
            // merge.
            query = query.filter(timed_after(AT, AT, ID, Rank::Ticket, c, self.tz, true));
        }
        query = query.order((tickets::created_at.desc(), tickets::id.desc()));
        if let Some(fetch) = self.fetch {
            // One more than the others: the cursor's own ticket may come back
            // and be dropped.
            query = query.limit(fetch + 1);
        }
        let rows: Vec<(i32, i32, String, i32, DateTime<Utc>, Option<DateTime<Utc>>)> = query
            .select((
                tickets::id,
                tickets::number,
                tickets::title,
                tickets::workflow_state_id,
                tickets::created_at,
                tickets::closed_at,
            ))
            .load(conn)?;
        let more = self.fetch.is_some_and(|f| rows.len() as i64 > f);
        if rows.is_empty() {
            return Ok((Vec::new(), more));
        }
        let ids: Vec<i32> = rows.iter().map(|r| r.0).collect();
        let links: Vec<(i32, i32, DateTime<Utc>)> = ticket_assets::table
            .filter(ticket_assets::ticket_id.eq_any(&ids))
            .filter(ticket_assets::asset_id.eq_any(self.asset_ids))
            .select((
                ticket_assets::ticket_id,
                ticket_assets::asset_id,
                ticket_assets::created_at,
            ))
            .load(conn)?;
        let state_ids: Vec<i32> = rows.iter().map(|r| r.3).collect();
        let states: HashMap<i32, TicketState> = workflow_states::table
            .filter(workflow_states::id.eq_any(&state_ids))
            .select((
                workflow_states::id,
                workflow_states::name,
                workflow_states::category,
            ))
            .load::<(i32, String, WorkflowStateCategory)>(conn)?
            .into_iter()
            .map(|(id, name, category)| (id, TicketState { id, name, category }))
            .collect();
        let by_id: HashMap<i32, _> = rows.into_iter().map(|r| (r.0, r)).collect();
        let entries = links
            .into_iter()
            .filter_map(|(ticket_id, asset_id, linked_at)| {
                let (id, number, title, state_id, created_at, closed_at) = by_id.get(&ticket_id)?;
                Some(Entry {
                    key: EntryKey {
                        day: self.day_of(*created_at),
                        at: Some(*created_at),
                        recorded_at: *created_at,
                        rank: Rank::Ticket,
                        id: *id,
                        asset_id,
                    },
                    data: EntryData::Ticket(TimelineTicket {
                        id: *id,
                        number: *number,
                        title: title.clone(),
                        state: states.get(state_id).cloned(),
                        created_at: *created_at,
                        closed_at: *closed_at,
                        linked_at,
                    }),
                })
            })
            .collect();
        Ok((entries, more))
    }

    /// Each asset's purchase date and when it was added, as they fall in the
    /// window and after the cursor.
    fn markers(&self, conn: &mut DbConnection) -> QueryResult<(Vec<Entry>, bool)> {
        let rows: Vec<(i32, Option<NaiveDate>, DateTime<Utc>)> = assets::table
            .filter(assets::id.eq_any(self.asset_ids))
            .select((assets::id, assets::purchase_date, assets::created_at))
            .load(conn)?;
        let mut entries = Vec::new();
        for (asset_id, purchased, created_at) in rows {
            if let Some(day) = purchased {
                entries.push(Entry {
                    key: EntryKey {
                        day,
                        at: None,
                        recorded_at: created_at,
                        rank: Rank::Marker,
                        id: 0,
                        asset_id,
                    },
                    data: EntryData::Marker(Marker::Purchased),
                });
            }
            entries.push(Entry {
                key: EntryKey {
                    day: self.day_of(created_at),
                    at: Some(created_at),
                    recorded_at: created_at,
                    rank: Rank::Marker,
                    id: 1,
                    asset_id,
                },
                data: EntryData::Marker(Marker::Added),
            });
        }
        entries.retain(|e| {
            self.filter.from.is_none_or(|from| e.key.day >= from)
                && self.filter.to.is_none_or(|to| e.key.day <= to)
                && self.after.is_none_or(|c| e.key < *c)
        });
        Ok((entries, false))
    }
}

/// Fold each disposal into the status entry that disposed of its asset and,
/// for staff, name the records each transition opened.
fn attach_status_details(
    conn: &mut DbConnection,
    entries: &mut [Entry],
    viewer: &Viewer,
) -> QueryResult<()> {
    let mut event_ids = Vec::new();
    let mut disposed_assets = Vec::new();
    for entry in entries.iter() {
        if let EntryData::Status { event, .. } = &entry.data {
            event_ids.push(event.id);
            if event.to_status == "disposed" {
                disposed_assets.push(event.asset_id);
            }
        }
    }
    if event_ids.is_empty() {
        return Ok(());
    }
    let disposals: Vec<AssetDisposal> = if disposed_assets.is_empty() {
        Vec::new()
    } else {
        asset_disposals::table
            .filter(asset_disposals::asset_id.eq_any(&disposed_assets))
            .order(asset_disposals::occurred_at.desc())
            .load(conn)?
    };
    let opened: Vec<(Option<i32>, i32)> = if viewer.staff {
        asset_records::table
            .filter(asset_records::lifecycle_event_id.eq_any(&event_ids))
            .select((asset_records::lifecycle_event_id, asset_records::id))
            .order(asset_records::id)
            .load(conn)?
    } else {
        Vec::new()
    };
    for entry in entries.iter_mut() {
        let EntryData::Status {
            event,
            disposal,
            record_ids,
        } = &mut entry.data
        else {
            continue;
        };
        if event.to_status == "disposed" {
            // The disposal written with this transition, else the asset's
            // (disposal is final, so there is one).
            *disposal = disposals
                .iter()
                .find(|d| d.lifecycle_event_id == Some(event.id))
                .or_else(|| {
                    disposals
                        .iter()
                        .find(|d| d.asset_id == event.asset_id && d.lifecycle_event_id.is_none())
                })
                .cloned();
        }
        *record_ids = opened
            .iter()
            .filter(|(e, _)| *e == Some(event.id))
            .map(|(_, id)| *id)
            .collect();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(day: &str, at: Option<&str>, rank: Rank, id: i32) -> EntryKey {
        EntryKey {
            day: day.parse().unwrap(),
            at: at.map(|a| a.parse().unwrap()),
            recorded_at: "2026-01-01T00:00:00Z".parse().unwrap(),
            rank,
            id,
            asset_id: 7,
        }
    }

    #[test]
    fn cursors_read_back_as_written() {
        for k in [
            key("2023-05-03", None, Rank::Record, 812),
            key(
                "2023-05-03",
                Some("2023-05-03T09:30:00.123456Z"),
                Rank::Status,
                1,
            ),
            key("1900-01-01", None, Rank::Marker, 0),
        ] {
            assert_eq!(EntryKey::decode(&k.encode()), Some(k));
        }
        assert_eq!(EntryKey::decode("nonsense"), None);
        assert_eq!(EntryKey::decode("2023-05-03~~1~9~1~1"), None);
        assert_eq!(EntryKey::decode("2023-05-03~~1~5~1~1~1"), None);
    }

    #[test]
    fn a_date_only_entry_sorts_below_the_timed_entries_of_its_day() {
        let date_only = key("2023-05-03", None, Rank::Status, 9);
        let early = key("2023-05-03", Some("2023-05-03T00:00:00Z"), Rank::Marker, 1);
        let day_before = key("2023-05-02", Some("2023-05-02T23:59:59Z"), Rank::Status, 1);
        assert!(early > date_only);
        assert!(date_only > day_before);
    }

    #[test]
    fn ties_fall_to_the_source_rank_then_the_id() {
        let at = Some("2023-05-03T09:30:00Z");
        let status = key("2023-05-03", at, Rank::Status, 1);
        let record = key("2023-05-03", at, Rank::Record, 2);
        let ticket = key("2023-05-03", at, Rank::Ticket, 3);
        let mut keys = vec![ticket, status, record];
        keys.sort_by(|a, b| b.cmp(a));
        assert_eq!(keys, vec![status, record, ticket]);
        assert!(key("2023-05-03", at, Rank::Record, 5) > key("2023-05-03", at, Rank::Record, 4));
    }
}
