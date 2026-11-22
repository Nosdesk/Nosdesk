//! Asset records: work done on an asset, entered as it happens or long after.
//!
//! A record never changes its asset's status. Nothing here reaches
//! `assets::set_status` or `asset_lifecycle::transition`: moving into or out
//! of repair stays a deliberate transition, which may write a record, while a
//! record never writes a transition.
//!
//! Every write validates the record as it will stand (the type applies to the
//! asset's kind, its fields, the type's required shared fields, the outcome,
//! the dates and the cost) and answers warnings beside the record: work dated
//! before the asset's purchase, a completion more than five years after the
//! work, or no way to tell who held the asset then.

use chrono::{DateTime, Months, NaiveDate, Utc};
use chrono_tz::Tz;
use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    Asset, AssetRecord, AssetRecordEdit, AssetRecordType, NewAssetRecord, OutcomeCategory,
    RecordSource, SnapshotSource, SyncAggregate, SyncOp,
};
use crate::repository::asset_media as media_repo;
use crate::schema::{
    asset_loans, asset_record_types, asset_records, assets, site_settings, tickets,
};
use crate::services::custom_fields::schema::{validate_attributes, AttributeError};
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;
use crate::utils::money;

/// The earliest day a record may be dated.
pub const EARLIEST_DAY: NaiveDate = match NaiveDate::from_ymd_opt(1900, 1, 1) {
    Some(day) => day,
    None => panic!("1900-01-01 is a date"),
};

/// How long after the work a completion is still plausible without a
/// warning.
const LONG_COMPLETION: Months = Months::new(60);

/// When the work happened or finished: a day, or a moment when the time is
/// known. A moment's day is taken in the workspace's timezone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkDate {
    Day(NaiveDate),
    At(DateTime<Utc>),
}

impl WorkDate {
    fn split(self, tz: Tz) -> (NaiveDate, Option<DateTime<Utc>>) {
        match self {
            WorkDate::Day(day) => (day, None),
            WorkDate::At(at) => (at.with_timezone(&tz).date_naive(), Some(at)),
        }
    }
}

/// A cost in the currency's minor units (cents for AUD, yen for JPY).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cost {
    pub minor: i64,
    pub currency: String,
}

/// Who held the asset and where it was when the work was done, as someone
/// entered it. Blank values are allowed: "nobody knows" is an answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub holder_uuid: Option<Uuid>,
    pub location: Option<String>,
}

/// A record to write.
#[derive(Debug, Clone)]
pub struct NewRecord {
    pub asset_id: i32,
    pub record_type_id: i32,
    pub summary: Option<String>,
    pub notes: Option<String>,
    pub occurred: WorkDate,
    /// `None` while the work is still open.
    pub completed: Option<WorkDate>,
    pub outcome: Option<String>,
    pub vendor: Option<String>,
    pub performed_by_uuid: Option<Uuid>,
    pub reference: Option<String>,
    pub cost: Option<Cost>,
    pub ticket_id: Option<i32>,
    pub replacement_asset_id: Option<i32>,
    /// `None` derives the holder and location from the asset's loans or, for
    /// work done today or yesterday, from the asset as it is.
    pub snapshot: Option<Snapshot>,
    pub attributes: Value,
    pub source: RecordSource,
    pub source_ref: Option<String>,
    pub import_job_id: Option<Uuid>,
    pub rule_id: Option<i32>,
    pub lifecycle_event_id: Option<i32>,
    /// Who entered it; `None` for a system writer.
    pub recorded_by: Option<Uuid>,
}

/// An edit. Outer `None` leaves a field as it is; for a field that may be
/// empty, `Some(None)` clears it.
#[derive(Debug, Clone, Default)]
pub struct RecordChanges {
    pub record_type_id: Option<i32>,
    pub summary: Option<Option<String>>,
    pub notes: Option<Option<String>>,
    pub occurred: Option<WorkDate>,
    pub completed: Option<Option<WorkDate>>,
    pub outcome: Option<Option<String>>,
    pub vendor: Option<Option<String>>,
    pub performed_by_uuid: Option<Option<Uuid>>,
    pub reference: Option<Option<String>>,
    pub cost: Option<Option<Cost>>,
    pub ticket_id: Option<Option<i32>>,
    pub replacement_asset_id: Option<Option<i32>>,
    /// Entered values replace the snapshot. Without them, moving the work to
    /// another day derives the snapshot again unless it was entered.
    pub snapshot: Option<Snapshot>,
    pub attributes: Option<Value>,
}

/// Something worth a second look that doesn't stop the write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum RecordWarning {
    /// The work is dated before the asset's purchase date.
    BeforePurchase { purchase_date: NaiveDate },
    /// The work was completed more than five years after it started.
    CompletedLongAfter,
    /// Nothing tells who held the asset on the day of the work.
    HolderUnknown,
}

/// A written record and its warnings.
#[derive(Debug, Clone)]
pub struct RecordWrite {
    pub record: AssetRecord,
    pub warnings: Vec<RecordWarning>,
}

/// Why a write was refused.
#[derive(Debug)]
pub enum RecordError {
    NotFound,
    AssetNotFound,
    RecordTypeNotFound,
    /// A new record, or a change of type, names an archived type.
    RecordTypeArchived,
    /// The type isn't offered for the asset's kind.
    RecordTypeNotForKind {
        kind: String,
    },
    /// The values don't fit the type's fields.
    Attributes(AttributeError),
    /// A shared field the type requires is empty.
    Required {
        field: &'static str,
    },
    /// The outcome isn't one of the type's.
    UnknownOutcome {
        outcome: String,
    },
    /// An outcome on work that is still open.
    OutcomeWhileOpen,
    /// A replacement asset without an outcome in the `replaced` category.
    ReplacementNeedsReplacedOutcome,
    ReplacementIsSelf,
    ReplacementNotFound,
    TicketNotFound,
    /// Dated before 1900-01-01.
    TooEarly,
    /// Dated after tomorrow in the workspace's timezone.
    InFuture,
    CompletedBeforeOccurred,
    UnknownCurrency {
        currency: String,
    },
    NegativeCost,
    /// The record changed since the version the edit read.
    VersionConflict {
        current: i32,
    },
    /// Another record already carries this `source_ref`.
    SourceRefExists {
        id: i32,
    },
    Db(Error),
}

impl From<Error> for RecordError {
    fn from(e: Error) -> Self {
        RecordError::Db(e)
    }
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordError::NotFound => write!(f, "record not found"),
            RecordError::AssetNotFound => write!(f, "asset not found"),
            RecordError::RecordTypeNotFound => write!(f, "record type not found"),
            RecordError::RecordTypeArchived => write!(f, "record type is archived"),
            RecordError::RecordTypeNotForKind { kind } => {
                write!(f, "record type isn't offered for assets of kind {kind}")
            }
            RecordError::Attributes(e) => write!(f, "{e}"),
            RecordError::Required { field } => write!(f, "{field} is required for this type"),
            RecordError::UnknownOutcome { outcome } => {
                write!(f, "{outcome} isn't an outcome of this type")
            }
            RecordError::OutcomeWhileOpen => write!(f, "open work has no outcome"),
            RecordError::ReplacementNeedsReplacedOutcome => {
                write!(f, "a replacement asset needs a replaced outcome")
            }
            RecordError::ReplacementIsSelf => write!(f, "an asset can't replace itself"),
            RecordError::ReplacementNotFound => write!(f, "replacement asset not found"),
            RecordError::TicketNotFound => write!(f, "ticket not found"),
            RecordError::TooEarly => write!(f, "dates start at 1900-01-01"),
            RecordError::InFuture => write!(f, "work can't be dated in the future"),
            RecordError::CompletedBeforeOccurred => {
                write!(f, "completion is before the work started")
            }
            RecordError::UnknownCurrency { currency } => write!(f, "unknown currency {currency}"),
            RecordError::NegativeCost => write!(f, "cost can't be negative"),
            RecordError::VersionConflict { current } => {
                write!(f, "the record changed since; it is at version {current}")
            }
            RecordError::SourceRefExists { id } => {
                write!(f, "record {id} already has this source reference")
            }
            RecordError::Db(e) => write!(f, "{e}"),
        }
    }
}

/// The `asset_record` sync payload: the whole row but its workspace.
pub fn record_sync_payload(row: &AssetRecord) -> Value {
    json!({
        "id": row.id,
        "asset_id": row.asset_id,
        "record_type_id": row.record_type_id,
        "summary": row.summary,
        "notes": row.notes,
        "occurred_on": row.occurred_on,
        "occurred_at": row.occurred_at,
        "completed_on": row.completed_on,
        "completed_at": row.completed_at,
        "outcome": row.outcome,
        "vendor": row.vendor,
        "performed_by_uuid": row.performed_by_uuid,
        "reference": row.reference,
        "cost_minor": row.cost_minor,
        "cost_currency": row.cost_currency,
        "ticket_id": row.ticket_id,
        "replacement_asset_id": row.replacement_asset_id,
        "holder_uuid": row.holder_uuid,
        "location": row.location,
        "snapshot_source": row.snapshot_source,
        "attributes": row.attributes,
        "plugin_fields": row.plugin_fields,
        "lifecycle_event_id": row.lifecycle_event_id,
        "schedule_id": row.schedule_id,
        "source": row.source,
        "source_ref": row.source_ref,
        "import_job_id": row.import_job_id,
        "rule_id": row.rule_id,
        "recorded_at": row.recorded_at,
        "recorded_by": row.recorded_by,
        "updated_at": row.updated_at,
        "updated_by": row.updated_by,
        "version": row.version,
    })
}

fn emit_record(
    conn: &mut DbConnection,
    op: SyncOp,
    event_type: &'static str,
    id: i32,
    data: Value,
) -> QueryResult<()> {
    emit::record(
        conn,
        SyncEmit {
            aggregate: SyncAggregate::AssetRecord,
            aggregate_id: id.to_string(),
            op,
            event_type,
            data,
            groups: groups::workspace(),
            causation_id: None,
        },
    )?;
    Ok(())
}

pub fn get(conn: &mut DbConnection, id: i32) -> QueryResult<Option<AssetRecord>> {
    asset_records::table.find(id).first(conn).optional()
}

/// An asset's records, newest work first: by day, then the timed ones of a
/// day before its date-only ones.
pub fn list_for_asset(conn: &mut DbConnection, asset_id: i32) -> QueryResult<Vec<AssetRecord>> {
    asset_records::table
        .filter(asset_records::asset_id.eq(asset_id))
        .order((
            asset_records::occurred_on.desc(),
            asset_records::occurred_at.desc().nulls_last(),
            asset_records::id.desc(),
        ))
        .load(conn)
}

/// The record carrying `source_ref`, if any.
pub fn find_by_source_ref(
    conn: &mut DbConnection,
    source_ref: &str,
) -> QueryResult<Option<AssetRecord>> {
    asset_records::table
        .filter(asset_records::source_ref.eq(source_ref))
        .first(conn)
        .optional()
}

/// The workspace's timezone, which turns a moment into the day of the work.
pub fn workspace_timezone(conn: &mut DbConnection) -> QueryResult<Tz> {
    let zone: Option<String> = site_settings::table
        .select(site_settings::default_timezone)
        .first(conn)
        .optional()?;
    Ok(crate::utils::locale::effective_timezone(
        None,
        zone.as_deref().unwrap_or("UTC"),
    ))
}

fn load_type(conn: &mut DbConnection, id: i32) -> Result<AssetRecordType, RecordError> {
    asset_record_types::table
        .find(id)
        .first(conn)
        .optional()?
        .ok_or(RecordError::RecordTypeNotFound)
}

fn load_asset(conn: &mut DbConnection, id: i32) -> Result<Asset, RecordError> {
    assets::table
        .find(id)
        .first(conn)
        .optional()?
        .ok_or(RecordError::AssetNotFound)
}

/// Text with its ends trimmed; empty is none.
fn clean(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// Write a record. It never touches the asset.
pub fn create(conn: &mut DbConnection, input: NewRecord) -> Result<RecordWrite, RecordError> {
    // emit::record fires inside emit_record.
    conn.transaction::<RecordWrite, RecordError, _>(|conn| {
        let tz = workspace_timezone(conn)?;
        let asset = load_asset(conn, input.asset_id)?;
        let rtype = load_type(conn, input.record_type_id)?;
        if rtype.archived_at.is_some() {
            return Err(RecordError::RecordTypeArchived);
        }
        if !rtype.applies_to(&asset.kind) {
            return Err(RecordError::RecordTypeNotForKind { kind: asset.kind });
        }
        let source_ref = clean(input.source_ref);
        if let Some(existing) = match &source_ref {
            Some(r) => find_by_source_ref(conn, r)?,
            None => None,
        } {
            return Err(RecordError::SourceRefExists { id: existing.id });
        }

        let (occurred_on, occurred_at) = input.occurred.split(tz);
        let (completed_on, completed_at) = match input.completed.map(|c| c.split(tz)) {
            Some((on, at)) => (Some(on), at),
            None => (None, None),
        };
        let (cost_minor, cost_currency) = match input.cost {
            Some(c) => (Some(c.minor), Some(c.currency.trim().to_ascii_uppercase())),
            None => (None, None),
        };
        let snapshot = match input.snapshot {
            Some(s) => (s.holder_uuid, clean(s.location), SnapshotSource::Entered),
            None => derive_snapshot(conn, &asset, occurred_on, tz)?,
        };
        let draft = AssetRecordEdit {
            record_type_id: rtype.id,
            summary: clean(input.summary),
            notes: clean(input.notes),
            occurred_on,
            occurred_at,
            completed_on,
            completed_at,
            outcome: clean(input.outcome),
            vendor: clean(input.vendor),
            performed_by_uuid: input.performed_by_uuid,
            reference: clean(input.reference),
            cost_minor,
            cost_currency,
            ticket_id: input.ticket_id,
            replacement_asset_id: input.replacement_asset_id,
            holder_uuid: snapshot.0,
            location: snapshot.1,
            snapshot_source: snapshot.2.as_str().to_string(),
            attributes: input.attributes,
            updated_at: Utc::now(),
            updated_by: input.recorded_by,
            version: 1,
        };
        let warnings = check(conn, &draft, &asset, &rtype, tz)?;

        let row: AssetRecord = diesel::insert_into(asset_records::table)
            .values(&NewAssetRecord {
                asset_id: asset.id,
                record_type_id: draft.record_type_id,
                summary: draft.summary,
                notes: draft.notes,
                occurred_on: draft.occurred_on,
                occurred_at: draft.occurred_at,
                completed_on: draft.completed_on,
                completed_at: draft.completed_at,
                outcome: draft.outcome,
                vendor: draft.vendor,
                performed_by_uuid: draft.performed_by_uuid,
                reference: draft.reference,
                cost_minor: draft.cost_minor,
                cost_currency: draft.cost_currency,
                ticket_id: draft.ticket_id,
                replacement_asset_id: draft.replacement_asset_id,
                holder_uuid: draft.holder_uuid,
                location: draft.location,
                snapshot_source: draft.snapshot_source,
                attributes: draft.attributes,
                lifecycle_event_id: input.lifecycle_event_id,
                source: input.source.as_str().to_string(),
                source_ref,
                import_job_id: input.import_job_id,
                rule_id: input.rule_id,
                recorded_by: input.recorded_by,
                updated_by: input.recorded_by,
            })
            .get_result(conn)?;
        emit_record(
            conn,
            SyncOp::Insert,
            "asset_record.created",
            row.id,
            record_sync_payload(&row),
        )?;
        Ok(RecordWrite {
            record: row,
            warnings,
        })
    })
}

/// Edit record `id` as it stood at `expected_version`. Bumps the version and
/// stamps the editor. The asset, the entry clock and author, and where the
/// record came from never change.
pub fn update(
    conn: &mut DbConnection,
    id: i32,
    expected_version: i32,
    changes: RecordChanges,
    updated_by: Option<Uuid>,
) -> Result<RecordWrite, RecordError> {
    // emit::record fires inside emit_record.
    conn.transaction::<RecordWrite, RecordError, _>(|conn| {
        let current: AssetRecord = asset_records::table
            .find(id)
            .for_no_key_update()
            .first(conn)
            .optional()?
            .ok_or(RecordError::NotFound)?;
        if current.version != expected_version {
            return Err(RecordError::VersionConflict {
                current: current.version,
            });
        }
        let tz = workspace_timezone(conn)?;
        let asset = load_asset(conn, current.asset_id)?;
        let rtype = load_type(
            conn,
            changes.record_type_id.unwrap_or(current.record_type_id),
        )?;
        if rtype.id != current.record_type_id {
            if rtype.archived_at.is_some() {
                return Err(RecordError::RecordTypeArchived);
            }
            if !rtype.applies_to(&asset.kind) {
                return Err(RecordError::RecordTypeNotForKind { kind: asset.kind });
            }
        }

        let (occurred_on, occurred_at) = match changes.occurred {
            Some(w) => w.split(tz),
            None => (current.occurred_on, current.occurred_at),
        };
        let (completed_on, completed_at) = match changes.completed {
            Some(Some(w)) => {
                let (on, at) = w.split(tz);
                (Some(on), at)
            }
            Some(None) => (None, None),
            None => (current.completed_on, current.completed_at),
        };
        let (cost_minor, cost_currency) = match changes.cost {
            Some(Some(c)) => (Some(c.minor), Some(c.currency.trim().to_ascii_uppercase())),
            Some(None) => (None, None),
            None => (current.cost_minor, current.cost_currency.clone()),
        };
        let entered = current.snapshot_source == SnapshotSource::Entered.as_str();
        let snapshot = match changes.snapshot {
            Some(s) => (s.holder_uuid, clean(s.location), SnapshotSource::Entered),
            None if !entered && occurred_on != current.occurred_on => {
                derive_snapshot(conn, &asset, occurred_on, tz)?
            }
            None => (
                current.holder_uuid,
                current.location.clone(),
                snapshot_source(&current.snapshot_source),
            ),
        };
        let draft = AssetRecordEdit {
            record_type_id: rtype.id,
            summary: changes.summary.map_or(current.summary, clean),
            notes: changes.notes.map_or(current.notes, clean),
            occurred_on,
            occurred_at,
            completed_on,
            completed_at,
            outcome: changes.outcome.map_or(current.outcome, clean),
            vendor: changes.vendor.map_or(current.vendor, clean),
            performed_by_uuid: changes
                .performed_by_uuid
                .unwrap_or(current.performed_by_uuid),
            reference: changes.reference.map_or(current.reference, clean),
            cost_minor,
            cost_currency,
            ticket_id: changes.ticket_id.unwrap_or(current.ticket_id),
            replacement_asset_id: changes
                .replacement_asset_id
                .unwrap_or(current.replacement_asset_id),
            holder_uuid: snapshot.0,
            location: snapshot.1,
            snapshot_source: snapshot.2.as_str().to_string(),
            attributes: changes.attributes.unwrap_or(current.attributes),
            updated_at: Utc::now(),
            updated_by,
            version: current.version + 1,
        };
        let warnings = check(conn, &draft, &asset, &rtype, tz)?;

        let row: AssetRecord = diesel::update(asset_records::table.find(id))
            .set(&draft)
            .get_result(conn)?;
        emit_record(
            conn,
            SyncOp::Update,
            "asset_record.updated",
            row.id,
            record_sync_payload(&row),
        )?;
        Ok(RecordWrite {
            record: row,
            warnings,
        })
    })
}

/// Delete record `id`. Its files stay on the asset, no longer pointing at
/// it. Returns the record as it was, or `None` when there was none.
pub fn delete(conn: &mut DbConnection, id: i32) -> QueryResult<Option<AssetRecord>> {
    // emit::record fires inside emit_record.
    conn.transaction::<Option<AssetRecord>, Error, _>(|conn| {
        let Some(row) = asset_records::table
            .find(id)
            .for_no_key_update()
            .first::<AssetRecord>(conn)
            .optional()?
        else {
            return Ok(None);
        };
        media_repo::set_record_files(conn, row.asset_id, row.id, &[])?;
        diesel::delete(asset_records::table.find(id)).execute(conn)?;
        emit_record(
            conn,
            SyncOp::Delete,
            "asset_record.deleted",
            id,
            json!({ "id": id, "asset_id": row.asset_id }),
        )?;
        Ok(Some(row))
    })
}

/// Make `media_ids` record `id`'s files (asset documents and photos that
/// point at it). An id that isn't one of the asset's files is left out of
/// what comes back.
pub fn set_files(
    conn: &mut DbConnection,
    id: i32,
    media_ids: &[i32],
) -> Result<Vec<crate::models::AssetMedia>, RecordError> {
    let row = get(conn, id)?.ok_or(RecordError::NotFound)?;
    Ok(media_repo::set_record_files(
        conn,
        row.asset_id,
        row.id,
        media_ids,
    )?)
}

fn snapshot_source(stored: &str) -> SnapshotSource {
    match stored {
        "entered" => SnapshotSource::Entered,
        "loan" => SnapshotSource::Loan,
        "current" => SnapshotSource::Current,
        _ => SnapshotSource::None,
    }
}

/// The holder and location on `day`, and where they came from: the loan
/// whose span covers the day gives its borrower; for work done today or
/// yesterday, the asset as it is now gives both; else nothing.
fn derive_snapshot(
    conn: &mut DbConnection,
    asset: &Asset,
    day: NaiveDate,
    tz: Tz,
) -> QueryResult<(Option<Uuid>, Option<String>, SnapshotSource)> {
    let start = day_start(day, tz);
    let end = day.succ_opt().map(|next| day_start(next, tz));
    let mut loans = asset_loans::table
        .filter(asset_loans::asset_id.eq(asset.id))
        .filter(asset_loans::borrower_user_uuid.is_not_null())
        .filter(
            asset_loans::returned_at
                .is_null()
                .or(asset_loans::returned_at.ge(start)),
        )
        .into_boxed();
    if let Some(end) = end {
        loans = loans.filter(asset_loans::loaned_at.lt(end));
    }
    let borrower: Option<Option<Uuid>> = loans
        .order(asset_loans::loaned_at.desc())
        .select(asset_loans::borrower_user_uuid)
        .first(conn)
        .optional()?;
    if let Some(Some(borrower)) = borrower {
        return Ok((Some(borrower), None, SnapshotSource::Loan));
    }
    let today = Utc::now().with_timezone(&tz).date_naive();
    if day == today || day.succ_opt() == Some(today) {
        return Ok((
            asset.primary_user_uuid,
            asset.location.clone(),
            SnapshotSource::Current,
        ));
    }
    Ok((None, None, SnapshotSource::None))
}

/// The first moment of `day` in `tz`.
fn day_start(day: NaiveDate, tz: Tz) -> DateTime<Utc> {
    use chrono::TimeZone;
    let midnight = day.and_hms_opt(0, 0, 0).unwrap_or_default();
    tz.from_local_datetime(&midnight)
        .earliest()
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|| midnight.and_utc())
}

/// Validate a record as it will stand, and the warnings it carries.
fn check(
    conn: &mut DbConnection,
    draft: &AssetRecordEdit,
    asset: &Asset,
    rtype: &AssetRecordType,
    tz: Tz,
) -> Result<Vec<RecordWarning>, RecordError> {
    // Dates: none before the floor, none after tomorrow (timezone slack), and
    // no completion before the work.
    let today = Utc::now().with_timezone(&tz).date_naive();
    let latest = today.succ_opt().unwrap_or(today);
    for day in std::iter::once(draft.occurred_on).chain(draft.completed_on) {
        if day < EARLIEST_DAY {
            return Err(RecordError::TooEarly);
        }
        if day > latest {
            return Err(RecordError::InFuture);
        }
    }
    if let Some(completed_on) = draft.completed_on {
        if completed_on < draft.occurred_on {
            return Err(RecordError::CompletedBeforeOccurred);
        }
        if let (Some(from), Some(to)) = (draft.occurred_at, draft.completed_at) {
            if to < from {
                return Err(RecordError::CompletedBeforeOccurred);
            }
        }
    }

    // The type's fields.
    validate_attributes(&rtype.field_schema, &draft.attributes).map_err(RecordError::Attributes)?;

    // The outcome: one of the type's, and only once the work is complete.
    let outcome = match &draft.outcome {
        Some(key) => {
            if draft.completed_on.is_none() {
                return Err(RecordError::OutcomeWhileOpen);
            }
            Some(
                rtype
                    .outcome(key)
                    .ok_or_else(|| RecordError::UnknownOutcome {
                        outcome: key.clone(),
                    })?,
            )
        }
        None => None,
    };

    // The shared fields the type requires. A required outcome is asked for
    // only once the work is complete.
    let present = [
        (
            "vendor",
            draft.vendor.is_some() || draft.performed_by_uuid.is_some(),
        ),
        ("reference", draft.reference.is_some()),
        ("cost", draft.cost_minor.is_some()),
        (
            "outcome",
            draft.outcome.is_some() || draft.completed_on.is_none(),
        ),
        ("ticket", draft.ticket_id.is_some()),
        ("holder", draft.holder_uuid.is_some()),
        ("location", draft.location.is_some()),
        ("summary", draft.summary.is_some()),
    ];
    for (field, is_present) in present {
        if !is_present && rtype.core_field(field) == crate::models::CoreFieldMode::Required {
            return Err(RecordError::Required { field });
        }
    }

    // A replacement names another asset here, and only for a replaced
    // outcome.
    if let Some(replacement) = draft.replacement_asset_id {
        if replacement == asset.id {
            return Err(RecordError::ReplacementIsSelf);
        }
        if outcome.as_ref().map(|o| o.category) != Some(OutcomeCategory::Replaced) {
            return Err(RecordError::ReplacementNeedsReplacedOutcome);
        }
        let exists: Option<i32> = assets::table
            .find(replacement)
            .select(assets::id)
            .first(conn)
            .optional()?;
        if exists.is_none() {
            return Err(RecordError::ReplacementNotFound);
        }
    }

    if let Some(ticket_id) = draft.ticket_id {
        let exists: Option<i32> = tickets::table
            .find(ticket_id)
            .select(tickets::id)
            .first(conn)
            .optional()?;
        if exists.is_none() {
            return Err(RecordError::TicketNotFound);
        }
    }

    if let (Some(minor), Some(code)) = (draft.cost_minor, draft.cost_currency.as_deref()) {
        if minor < 0 {
            return Err(RecordError::NegativeCost);
        }
        if money::currency(code).is_none() {
            return Err(RecordError::UnknownCurrency {
                currency: code.to_string(),
            });
        }
    }

    let mut warnings = Vec::new();
    if let Some(purchase_date) = asset.purchase_date {
        if draft.occurred_on < purchase_date {
            warnings.push(RecordWarning::BeforePurchase { purchase_date });
        }
    }
    if let Some(completed_on) = draft.completed_on {
        if draft
            .occurred_on
            .checked_add_months(LONG_COMPLETION)
            .is_some_and(|limit| completed_on > limit)
        {
            warnings.push(RecordWarning::CompletedLongAfter);
        }
    }
    if draft.snapshot_source == SnapshotSource::None.as_str() {
        warnings.push(RecordWarning::HolderUnknown);
    }
    Ok(warnings)
}
