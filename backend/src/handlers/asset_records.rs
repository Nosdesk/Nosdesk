//! Asset records over REST: the work done on an asset.
//!
//! - `GET    /api/assets/{id}/records`: a page of the asset's records, newest
//!   work first, filtered by type, outcome, open or completed, and day.
//! - `POST   /api/assets/{id}/records`: write one.
//! - `GET | PATCH | DELETE /api/assets/{id}/records/{record_id}`.
//! - `PUT    /api/assets/{id}/records/{record_id}/media`: which of the
//!   asset's files are the record's.
//! - `GET    /api/assets/record-vendors`: vendors already written, for
//!   suggestions.
//!
//! Records are staff-only whatever the requester asset setting: anyone else
//! gets 404 here, as if there were none. The repository validates every
//! write; this module reads bodies and maps its refusals to statuses. A
//! record is deleted by a workspace admin, or by whoever entered it within a
//! day of entering it.

use std::collections::{HashMap, HashSet};

use actix_web::http::StatusCode;
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::error;
use uuid::Uuid;

use crate::errors;
use crate::extractors::{AuthContext, ScopedStorage, TenantConn};
use crate::middleware::api_token::ApiTokenRequest;
use crate::models::{AssetMedia, AssetRecord, AssetRecordType, RecordSource};
use crate::repository::asset_records::{
    self as repo, Cost, NewRecord, RecordChanges, RecordCursor, RecordError, RecordFilter,
    RecordWrite, Snapshot, WorkDate,
};
use crate::repository::ticket_visibility::{self, VisibilityContext};
use crate::repository::{
    asset_media as media_repo, asset_record_types as types_repo, assets as assets_repo,
    site_settings as settings_repo,
};
use crate::utils::money::{self, DecimalMark};

/// How long whoever entered a record may delete it themselves.
const OWN_DELETE_WINDOW: Duration = Duration::hours(24);

const PAGE_DEFAULT: i64 = 100;
const PAGE_MAX: i64 = 200;
const VENDORS_DEFAULT: usize = 20;
const VENDORS_MAX: usize = 100;

/// A record type named by its id or its slug.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TypeRef {
    Id(i32),
    Slug(String),
}

/// A cost as written: a decimal amount and, optionally, its ISO 4217 code.
/// Without a code the amount is in the workspace currency.
#[derive(Debug, Clone, Deserialize)]
pub struct CostBody {
    pub amount: Value,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub record_type: TypeRef,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// The day of the work, or `occurred_at` when the time is known.
    #[serde(default)]
    pub occurred_on: Option<NaiveDate>,
    #[serde(default)]
    pub occurred_at: Option<DateTime<Utc>>,
    /// Absent while the work is still open.
    #[serde(default)]
    pub completed_on: Option<NaiveDate>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub performed_by_uuid: Option<Uuid>,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub cost: Option<CostBody>,
    #[serde(default)]
    pub ticket_id: Option<i32>,
    #[serde(default)]
    pub replacement_asset_id: Option<i32>,
    /// Who held the asset and where it was then. Sending either, even as
    /// `null`, records them as entered; sending neither derives them.
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub holder_uuid: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub location: Option<Option<String>>,
    #[serde(default)]
    pub attributes: Option<Value>,
    /// The source system's id, which makes the write idempotent.
    #[serde(default)]
    pub source_ref: Option<String>,
}

/// An edit to the record as it stood at `expected_version`. A field left
/// out keeps its value; `null` clears a field that may be empty.
#[derive(Debug, Deserialize)]
pub struct UpdateBody {
    pub expected_version: i32,
    #[serde(default)]
    pub record_type: Option<TypeRef>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub summary: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub notes: Option<Option<String>>,
    #[serde(default)]
    pub occurred_on: Option<NaiveDate>,
    #[serde(default)]
    pub occurred_at: Option<DateTime<Utc>>,
    /// `null` on both reopens the work.
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub completed_on: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub completed_at: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub outcome: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub vendor: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub performed_by_uuid: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub reference: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub cost: Option<Option<CostBody>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub ticket_id: Option<Option<i32>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub replacement_asset_id: Option<Option<i32>>,
    /// Sending either records both as entered; the one left out keeps its
    /// value.
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub holder_uuid: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub location: Option<Option<String>>,
    #[serde(default)]
    pub attributes: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// A type's slug or id.
    #[serde(rename = "type")]
    pub record_type: Option<String>,
    pub outcome: Option<String>,
    /// `true` for open work only, `false` for completed only.
    pub open: Option<bool>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    /// The `next_cursor` of the page before.
    pub before: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteQuery {
    /// Delete the record's files too, rather than leave them on the asset.
    #[serde(default)]
    pub delete_files: bool,
}

#[derive(Debug, Deserialize)]
pub struct MediaBody {
    pub media_ids: Vec<i32>,
}

#[derive(Debug, Deserialize)]
pub struct VendorQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

/// Why a request is refused, by this module or the repository.
#[derive(Debug)]
enum Refusal {
    /// Not staff: records don't exist for them.
    Hidden,
    AssetNotFound,
    BadRequest(String),
    Forbidden(&'static str),
    /// Media ids that aren't files of the record's asset.
    NotTheAssetsFiles(Vec<i32>),
    /// A person field names someone who isn't in the workspace (or no one):
    /// the same answer either way.
    UnknownPerson(&'static str),
    Record(RecordError),
}

impl From<diesel::result::Error> for Refusal {
    fn from(e: diesel::result::Error) -> Self {
        Refusal::Record(RecordError::Db(e))
    }
}

impl From<RecordError> for Refusal {
    fn from(e: RecordError) -> Self {
        Refusal::Record(e)
    }
}

fn unprocessable(code: &str, message: impl Into<String>, extra: Value) -> HttpResponse {
    errors::with_fields(StatusCode::UNPROCESSABLE_ENTITY, code, message, extra)
}

fn respond(refusal: Refusal) -> HttpResponse {
    match refusal {
        Refusal::Hidden => errors::not_found_msg("Not found"),
        Refusal::AssetNotFound => errors::not_found_msg("Asset not found"),
        Refusal::BadRequest(message) => errors::bad_request(message),
        Refusal::Forbidden(message) => errors::forbidden(message),
        Refusal::NotTheAssetsFiles(ids) => unprocessable(
            "MEDIA_NOT_ON_ASSET",
            "Some of these files aren't the asset's",
            json!({ "media_ids": ids }),
        ),
        Refusal::UnknownPerson(field) => unknown_person(field),
        Refusal::Record(e) => record_error(e),
    }
}

fn unknown_person(field: &str) -> HttpResponse {
    unprocessable(
        "UNKNOWN_PERSON",
        format!("{field} isn't someone in this workspace"),
        json!({ "field": field }),
    )
}

/// The status and body for each of the repository's refusals.
fn record_error(e: RecordError) -> HttpResponse {
    let message = e.to_string();
    match e {
        RecordError::NotFound => errors::not_found_msg("Record not found"),
        RecordError::AssetNotFound => errors::not_found_msg("Asset not found"),
        RecordError::RecordTypeNotFound => {
            unprocessable("RECORD_TYPE_NOT_FOUND", message, json!({}))
        }
        RecordError::RecordTypeArchived => {
            unprocessable("RECORD_TYPE_ARCHIVED", message, json!({}))
        }
        RecordError::RecordTypeNotForKind { kind } => {
            unprocessable("RECORD_TYPE_NOT_FOR_KIND", message, json!({ "kind": kind }))
        }
        RecordError::Attributes(_) => unprocessable("INVALID_ATTRIBUTES", message, json!({})),
        RecordError::Required { field } => {
            unprocessable("FIELD_REQUIRED", message, json!({ "field": field }))
        }
        RecordError::UnknownOutcome { outcome } => {
            unprocessable("UNKNOWN_OUTCOME", message, json!({ "outcome": outcome }))
        }
        RecordError::OutcomeWhileOpen => unprocessable("OUTCOME_WHILE_OPEN", message, json!({})),
        RecordError::ReplacementNeedsReplacedOutcome => {
            unprocessable("REPLACEMENT_NEEDS_REPLACED_OUTCOME", message, json!({}))
        }
        RecordError::ReplacementIsSelf => unprocessable("REPLACEMENT_IS_SELF", message, json!({})),
        RecordError::ReplacementNotFound => {
            unprocessable("REPLACEMENT_NOT_FOUND", message, json!({}))
        }
        RecordError::TicketNotFound => unprocessable("TICKET_NOT_FOUND", message, json!({})),
        RecordError::TooEarly => unprocessable("DATE_TOO_EARLY", message, json!({})),
        RecordError::InFuture => unprocessable("DATE_IN_FUTURE", message, json!({})),
        RecordError::CompletedBeforeOccurred => {
            unprocessable("COMPLETED_BEFORE_OCCURRED", message, json!({}))
        }
        RecordError::UnknownCurrency { currency } => {
            unprocessable("UNKNOWN_CURRENCY", message, json!({ "currency": currency }))
        }
        RecordError::NegativeCost => unprocessable("NEGATIVE_COST", message, json!({})),
        RecordError::VersionConflict { current } => errors::with_fields(
            StatusCode::CONFLICT,
            "VERSION_CONFLICT",
            message,
            json!({ "current_version": current }),
        ),
        RecordError::SourceRefExists { id } => errors::with_fields(
            StatusCode::CONFLICT,
            "RECORD_EXISTS",
            message,
            json!({ "id": id }),
        ),
        RecordError::TooLong { field, max } => {
            unprocessable("TOO_LONG", message, json!({ "field": field, "max": max }))
        }
        RecordError::Db(e) => {
            if let diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::ForeignKeyViolation,
                info,
            ) = &e
            {
                if let Some(answer) = foreign_key_answer(info.constraint_name()) {
                    return answer;
                }
            }
            error!(error = ?e, "asset record request failed");
            errors::internal("Failed to process the asset record")
        }
    }
}

/// The answer for a write a foreign key refused, by the key: something it
/// names went away between the checks and the write.
fn foreign_key_answer(constraint: Option<&str>) -> Option<HttpResponse> {
    let message = |what: &str| format!("{what} not found");
    Some(match constraint? {
        "asset_records_performed_by_uuid_fkey" => unknown_person("performed_by_uuid"),
        "asset_records_holder_uuid_fkey" => unknown_person("holder_uuid"),
        "asset_records_ticket_id_fkey" => {
            unprocessable("TICKET_NOT_FOUND", message("ticket"), json!({}))
        }
        "asset_records_replacement_asset_id_fkey" => unprocessable(
            "REPLACEMENT_NOT_FOUND",
            message("replacement asset"),
            json!({}),
        ),
        "asset_records_record_type_id_fkey" => {
            unprocessable("RECORD_TYPE_NOT_FOUND", message("record type"), json!({}))
        }
        "asset_records_asset_id_fkey" => errors::not_found_msg("Asset not found"),
        _ => return None,
    })
}

/// Only staff see records; to anyone else they don't exist.
fn require_staff(auth: &AuthContext) -> Result<(), Refusal> {
    if auth.can_handle_tickets() {
        Ok(())
    } else {
        Err(Refusal::Hidden)
    }
}

fn require_asset(conn: &mut crate::db::DbConnection, asset_id: i32) -> Result<(), Refusal> {
    match assets_repo::get_device_by_id(conn, asset_id) {
        Ok(_) => Ok(()),
        Err(diesel::result::Error::NotFound) => Err(Refusal::AssetNotFound),
        Err(e) => Err(e.into()),
    }
}

/// Record `record_id` when it is one of asset `asset_id`'s.
fn record_on_asset(
    conn: &mut crate::db::DbConnection,
    asset_id: i32,
    record_id: i32,
) -> Result<AssetRecord, Refusal> {
    match repo::get(conn, record_id)? {
        Some(r) if r.asset_id == asset_id => Ok(r),
        _ => Err(RecordError::NotFound.into()),
    }
}

fn resolve_type(
    conn: &mut crate::db::DbConnection,
    by: &TypeRef,
) -> Result<AssetRecordType, Refusal> {
    let found = match by {
        TypeRef::Id(id) => types_repo::get(conn, *id)?,
        // A slug, else digits as an id: `"7"` in a body reads as `?type=7`.
        TypeRef::Slug(raw) => match types_repo::get_by_slug(conn, raw.trim())? {
            Some(t) => Some(t),
            None => match raw.trim().parse::<i32>() {
                Ok(id) => types_repo::get(conn, id)?,
                Err(_) => None,
            },
        },
    };
    found.ok_or(Refusal::Record(RecordError::RecordTypeNotFound))
}

/// The people and ticket a write newly names: whoever did the work can
/// work tickets here, whoever held the asset is a member here, and the
/// ticket is one the caller can see. An unknown id and one outside the
/// workspace get the same answer.
fn check_references(
    conn: &mut crate::db::DbConnection,
    auth: &AuthContext,
    performed_by: Option<Uuid>,
    holder: Option<Uuid>,
    ticket_id: Option<i32>,
) -> Result<(), Refusal> {
    if let Some(user) = performed_by {
        if !repo::may_perform(conn, user)? {
            return Err(Refusal::UnknownPerson("performed_by_uuid"));
        }
    }
    if let Some(user) = holder {
        if !repo::may_hold(conn, user)? {
            return Err(Refusal::UnknownPerson("holder_uuid"));
        }
    }
    if let Some(ticket_id) = ticket_id {
        let viewer = VisibilityContext::from_auth(auth);
        if !ticket_visibility::can_view_ticket(conn, &viewer, ticket_id)? {
            return Err(RecordError::TicketNotFound.into());
        }
    }
    Ok(())
}

/// The value a PATCH field writes when it changes what is there; `None`
/// when it is left out, cleared, or the same.
fn newly<T: PartialEq + Copy>(field: Option<Option<T>>, current: Option<T>) -> Option<T> {
    field.flatten().filter(|v| Some(*v) != current)
}

/// The currency an amount without a code is in: the workspace currency,
/// else the one its default locale suggests.
fn workspace_currency(conn: &mut crate::db::DbConnection) -> Result<Option<String>, Refusal> {
    let settings = settings_repo::get_site_settings(conn)?;
    Ok(settings
        .currency_code
        .or_else(|| money::suggested_currency(&settings.default_locale).map(str::to_string)))
}

/// A cost body in minor units. The amount is a decimal string (or a JSON
/// number) with `.` as the decimal mark.
fn read_cost(body: CostBody, fallback: Option<String>) -> Result<Cost, Refusal> {
    let code = body
        .currency
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| !c.is_empty())
        .or(fallback)
        .ok_or_else(|| {
            Refusal::BadRequest(
                "cost.currency is required while the workspace has no currency".into(),
            )
        })?;
    let text = match body.amount {
        Value::String(s) => s,
        Value::Number(n) => n.to_string(),
        _ => {
            return Err(Refusal::BadRequest(
                "cost.amount must be a decimal string such as \"1250.50\"".into(),
            ))
        }
    };
    let minor = money::parse_amount(&text, &code, DecimalMark::Dot).map_err(|e| match e {
        money::MoneyError::UnknownCurrency(currency) => {
            Refusal::Record(RecordError::UnknownCurrency { currency })
        }
        other => Refusal::BadRequest(format!("cost.amount {other}")),
    })?;
    Ok(Cost {
        minor,
        currency: code,
    })
}

/// The day of the work from a body's two date fields, one of which it
/// gives.
fn work_date(
    on: Option<NaiveDate>,
    at: Option<DateTime<Utc>>,
    name: &str,
) -> Result<Option<WorkDate>, Refusal> {
    match (on, at) {
        (Some(_), Some(_)) => Err(Refusal::BadRequest(format!(
            "give {name}_on or {name}_at, not both"
        ))),
        (Some(day), None) => Ok(Some(WorkDate::Day(day))),
        (None, Some(at)) => Ok(Some(WorkDate::At(at))),
        (None, None) => Ok(None),
    }
}

/// What wrote a record over HTTP: an API token, or someone in the app.
fn source_of(req: &HttpRequest) -> RecordSource {
    if req.extensions().get::<ApiTokenRequest>().is_some() {
        RecordSource::Api
    } else {
        RecordSource::Manual
    }
}

/// A record as the API answers it: the row without its workspace, its
/// type's slug, the cost as `{ amount, currency, amount_minor }`, and its
/// files.
fn record_json(record: &AssetRecord, slug: Option<&str>, files: &[&AssetMedia]) -> Value {
    let cost = match (record.cost_minor, record.cost_currency.as_deref()) {
        (Some(minor), Some(code)) => json!({
            "amount": money::format_minor(minor, code).ok(),
            "currency": code,
            "amount_minor": minor,
        }),
        _ => Value::Null,
    };
    let mut out = repo::record_sync_payload(record);
    if let Value::Object(map) = &mut out {
        map.remove("cost_minor");
        map.remove("cost_currency");
        map.insert("record_type".into(), json!(slug));
        map.insert("cost".into(), cost);
        map.insert("files".into(), json!(files));
    }
    out
}

/// Records with their slugs and files, read in one pass.
fn records_json(
    conn: &mut crate::db::DbConnection,
    records: &[AssetRecord],
) -> Result<Vec<Value>, Refusal> {
    let slugs: HashMap<i32, String> = types_repo::list(conn, true)?
        .into_iter()
        .map(|t| (t.id, t.slug))
        .collect();
    let ids: Vec<i32> = records.iter().map(|r| r.id).collect();
    let files = media_repo::list_for_records(conn, &ids)?;
    Ok(records
        .iter()
        .map(|r| {
            let own: Vec<&AssetMedia> =
                files.iter().filter(|f| f.record_id == Some(r.id)).collect();
            record_json(r, slugs.get(&r.record_type_id).map(String::as_str), &own)
        })
        .collect())
}

fn write_json(conn: &mut crate::db::DbConnection, write: &RecordWrite) -> Result<Value, Refusal> {
    let record = records_json(conn, std::slice::from_ref(&write.record))?
        .pop()
        .unwrap_or(Value::Null);
    Ok(json!({ "record": record, "warnings": write.warnings }))
}

fn encode_cursor(c: &RecordCursor) -> String {
    let at = c
        .occurred_at
        .map(|at| at.timestamp_micros().to_string())
        .unwrap_or_default();
    format!("{}~{at}~{}", c.occurred_on, c.id)
}

fn decode_cursor(raw: &str) -> Option<RecordCursor> {
    let mut parts = raw.split('~');
    let occurred_on = parts.next()?.parse().ok()?;
    let at = parts.next()?;
    let occurred_at = if at.is_empty() {
        None
    } else {
        Some(DateTime::from_timestamp_micros(at.parse().ok()?)?)
    };
    let id = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(RecordCursor {
        occurred_on,
        occurred_at,
        id,
    })
}

/// `GET /api/assets/{id}/records`.
pub async fn list_for_asset(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    query: web::Query<ListQuery>,
) -> HttpResponse {
    let asset_id = path.into_inner();
    let query = query.into_inner();
    let after = match query.before.as_deref().filter(|c| !c.is_empty()) {
        Some(raw) => match decode_cursor(raw) {
            Some(c) => Some(c),
            None => return errors::bad_request("before isn't a cursor this list gave"),
        },
        None => None,
    };
    let limit = query.limit.unwrap_or(PAGE_DEFAULT).clamp(1, PAGE_MAX);
    let result = tc.run_result(|conn| {
        require_staff(&auth)?;
        require_asset(conn, asset_id)?;
        let record_type_id = match query.record_type.as_deref().filter(|t| !t.is_empty()) {
            Some(raw) => match resolve_type(conn, &TypeRef::Slug(raw.to_string())) {
                Ok(t) => Some(t.id),
                // No such type: no records of it.
                Err(Refusal::Record(RecordError::RecordTypeNotFound)) => {
                    return Ok(json!({ "records": [], "next_cursor": null }))
                }
                Err(e) => return Err(e),
            },
            None => None,
        };
        let filter = RecordFilter {
            record_type_id,
            outcome: query.outcome.clone().filter(|o| !o.is_empty()),
            open: query.open,
            from: query.from,
            to: query.to,
        };
        let (rows, next) = repo::list_page(conn, asset_id, &filter, after, limit)?;
        Ok(json!({
            "records": records_json(conn, &rows)?,
            "next_cursor": next.as_ref().map(encode_cursor),
        }))
    });
    match result {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(r) => respond(r),
    }
}

/// `POST /api/assets/{id}/records`.
pub async fn create(
    mut tc: TenantConn,
    auth: AuthContext,
    req: HttpRequest,
    path: web::Path<i32>,
    body: web::Json<CreateBody>,
) -> HttpResponse {
    let asset_id = path.into_inner();
    let body = body.into_inner();
    let source = source_of(&req);
    let result = tc.run_result(|conn| {
        require_staff(&auth)?;
        require_asset(conn, asset_id)?;
        let rtype = resolve_type(conn, &body.record_type)?;
        let occurred = work_date(body.occurred_on, body.occurred_at, "occurred")?
            .ok_or_else(|| Refusal::BadRequest("occurred_on or occurred_at is required".into()))?;
        let completed = work_date(body.completed_on, body.completed_at, "completed")?;
        let cost = match body.cost {
            Some(c) => Some(read_cost(c, workspace_currency(conn)?)?),
            None => None,
        };
        check_references(
            conn,
            &auth,
            body.performed_by_uuid,
            body.holder_uuid.flatten(),
            body.ticket_id,
        )?;
        let snapshot = (body.holder_uuid.is_some() || body.location.is_some()).then(|| Snapshot {
            holder_uuid: body.holder_uuid.flatten(),
            location: body.location.flatten(),
        });
        let write = repo::create(
            conn,
            NewRecord {
                asset_id,
                record_type_id: rtype.id,
                summary: body.summary,
                notes: body.notes,
                occurred,
                completed,
                outcome: body.outcome,
                vendor: body.vendor,
                performed_by_uuid: body.performed_by_uuid,
                reference: body.reference,
                cost,
                ticket_id: body.ticket_id,
                replacement_asset_id: body.replacement_asset_id,
                snapshot,
                attributes: body.attributes.unwrap_or_else(|| json!({})),
                source,
                source_ref: body.source_ref,
                import_job_id: None,
                rule_id: None,
                lifecycle_event_id: None,
                recorded_by: Some(auth.user_uuid),
            },
        )?;
        write_json(conn, &write)
    });
    match result {
        Ok(body) => HttpResponse::Created().json(body),
        Err(r) => respond(r),
    }
}

/// `GET /api/assets/{id}/records/{record_id}`.
pub async fn get(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<(i32, i32)>,
) -> HttpResponse {
    let (asset_id, record_id) = path.into_inner();
    let result = tc.run_result(|conn| {
        require_staff(&auth)?;
        let record = record_on_asset(conn, asset_id, record_id)?;
        Ok(records_json(conn, std::slice::from_ref(&record))?
            .pop()
            .unwrap_or(Value::Null))
    });
    match result {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(r) => respond(r),
    }
}

/// `PATCH /api/assets/{id}/records/{record_id}`.
pub async fn update(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<(i32, i32)>,
    body: web::Json<UpdateBody>,
) -> HttpResponse {
    let (asset_id, record_id) = path.into_inner();
    let body = body.into_inner();
    let result = tc.run_result(|conn| {
        require_staff(&auth)?;
        let current = record_on_asset(conn, asset_id, record_id)?;
        let record_type_id = match &body.record_type {
            Some(by) => Some(resolve_type(conn, by)?.id),
            None => None,
        };
        let occurred = work_date(body.occurred_on, body.occurred_at, "occurred")?;
        // Nulling `completed_on` reopens the work; nulling only
        // `completed_at` keeps the day and drops the time.
        let completed = match (body.completed_on, body.completed_at) {
            (Some(Some(_)), Some(Some(_))) => {
                return Err(Refusal::BadRequest(
                    "give completed_on or completed_at, not both".into(),
                ))
            }
            (Some(None), _) => Some(None),
            (_, Some(Some(at))) => Some(Some(WorkDate::At(at))),
            (Some(Some(day)), _) => Some(Some(WorkDate::Day(day))),
            (None, Some(None)) => current.completed_on.map(|day| Some(WorkDate::Day(day))),
            (None, None) => None,
        };
        check_references(
            conn,
            &auth,
            newly(body.performed_by_uuid, current.performed_by_uuid),
            newly(body.holder_uuid, current.holder_uuid),
            newly(body.ticket_id, current.ticket_id),
        )?;
        let cost = match body.cost {
            Some(Some(c)) => Some(Some(read_cost(c, workspace_currency(conn)?)?)),
            Some(None) => Some(None),
            None => None,
        };
        let snapshot = (body.holder_uuid.is_some() || body.location.is_some()).then(|| Snapshot {
            holder_uuid: body.holder_uuid.unwrap_or(current.holder_uuid),
            location: body.location.unwrap_or_else(|| current.location.clone()),
        });
        let write = repo::update(
            conn,
            record_id,
            body.expected_version,
            RecordChanges {
                record_type_id,
                summary: body.summary,
                notes: body.notes,
                occurred,
                completed,
                outcome: body.outcome,
                vendor: body.vendor,
                performed_by_uuid: body.performed_by_uuid,
                reference: body.reference,
                cost,
                ticket_id: body.ticket_id,
                replacement_asset_id: body.replacement_asset_id,
                snapshot,
                attributes: body.attributes,
            },
            Some(auth.user_uuid),
        )?;
        write_json(conn, &write)
    });
    match result {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(r) => respond(r),
    }
}

/// Whether `auth` may delete `record`: a workspace admin may; so may whoever
/// entered it, within a day of entering it.
fn may_delete(auth: &AuthContext, record: &AssetRecord, now: DateTime<Utc>) -> bool {
    auth.is_workspace_admin()
        || (record.recorded_by == Some(auth.user_uuid)
            && now - record.recorded_at < OWN_DELETE_WINDOW)
}

/// `DELETE /api/assets/{id}/records/{record_id}`. The record's files stay
/// on the asset unless `delete_files=true`.
pub async fn delete(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<(i32, i32)>,
    query: web::Query<DeleteQuery>,
    storage: ScopedStorage,
) -> HttpResponse {
    let (asset_id, record_id) = path.into_inner();
    let delete_files = query.delete_files;
    let result = tc.run_result(|conn| {
        require_staff(&auth)?;
        let record = record_on_asset(conn, asset_id, record_id)?;
        if !may_delete(&auth, &record, Utc::now()) {
            return Err(Refusal::Forbidden(
                "Only workspace admins, or whoever entered a record within a day of entering it, can delete it",
            ));
        }
        let files = if delete_files {
            media_repo::list_for_record(conn, record_id)?
        } else {
            Vec::new()
        };
        if repo::delete(conn, record_id)?.is_none() {
            return Err(RecordError::NotFound.into());
        }
        let mut deleted = Vec::with_capacity(files.len());
        for file in files {
            if let Some(row) = media_repo::delete(conn, file.id)? {
                deleted.push(row);
            }
        }
        Ok(deleted)
    });
    match result {
        Ok(files) => {
            for file in &files {
                crate::handlers::asset_media::remove_stored_objects(&storage, file).await;
            }
            HttpResponse::Ok().json(json!({ "deleted": true, "files_deleted": files.len() }))
        }
        Err(r) => respond(r),
    }
}

/// `PUT /api/assets/{id}/records/{record_id}/media`: make `media_ids`, all
/// files of the record's asset, the record's files. Refused whole when any
/// isn't the asset's.
pub async fn set_media(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<(i32, i32)>,
    body: web::Json<MediaBody>,
) -> HttpResponse {
    let (asset_id, record_id) = path.into_inner();
    let mut asked: Vec<i32> = body.into_inner().media_ids;
    asked.sort_unstable();
    asked.dedup();
    let result = tc.run_result(|conn| {
        require_staff(&auth)?;
        record_on_asset(conn, asset_id, record_id)?;
        let files = repo::set_files(conn, record_id, &asked)?;
        let linked: HashSet<i32> = files.iter().map(|f| f.id).collect();
        let missing: Vec<i32> = asked
            .iter()
            .copied()
            .filter(|id| !linked.contains(id))
            .collect();
        if !missing.is_empty() {
            // Rolls back the links already written.
            return Err(Refusal::NotTheAssetsFiles(missing));
        }
        Ok(files)
    });
    match result {
        Ok(files) => HttpResponse::Ok().json(files),
        Err(r) => respond(r),
    }
}

/// `GET /api/assets/record-vendors?q=`: vendors already written on records,
/// most used first.
pub async fn vendors(
    mut tc: TenantConn,
    auth: AuthContext,
    query: web::Query<VendorQuery>,
) -> HttpResponse {
    let query = query.into_inner();
    let limit = query.limit.unwrap_or(VENDORS_DEFAULT).clamp(1, VENDORS_MAX);
    let result = tc.run_result(|conn| {
        require_staff(&auth)?;
        Ok(repo::vendor_suggestions(conn, query.q.as_deref(), limit)?)
    });
    match result {
        Ok(vendors) => HttpResponse::Ok().json(vendors),
        Err(r) => respond(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_read_back_as_written() {
        let day = NaiveDate::from_ymd_opt(2023, 5, 3).unwrap();
        for at in [None, DateTime::from_timestamp_micros(1_683_072_000_123_456)] {
            let c = RecordCursor {
                occurred_on: day,
                occurred_at: at,
                id: 812,
            };
            assert_eq!(decode_cursor(&encode_cursor(&c)), Some(c));
        }
        assert_eq!(decode_cursor("2023-05-03~~"), None);
        assert_eq!(decode_cursor("nonsense"), None);
        assert_eq!(decode_cursor("2023-05-03~~1~2"), None);
    }

    #[test]
    fn costs_read_in_the_currency_given_or_the_workspace_one() {
        let cost = |amount: Value, currency: Option<&str>, fallback: Option<&str>| {
            read_cost(
                CostBody {
                    amount,
                    currency: currency.map(str::to_string),
                },
                fallback.map(str::to_string),
            )
        };
        let c = cost(json!("1250.50"), Some("aud"), None).unwrap();
        assert_eq!((c.minor, c.currency.as_str()), (125_050, "AUD"));
        let c = cost(json!("1250"), None, Some("JPY")).unwrap();
        assert_eq!((c.minor, c.currency.as_str()), (1250, "JPY"));
        let c = cost(json!(12.5), Some("USD"), None).unwrap();
        assert_eq!(c.minor, 1250);
        assert!(matches!(
            cost(json!("1.234"), Some("AUD"), None),
            Err(Refusal::BadRequest(_))
        ));
        assert!(matches!(
            cost(json!("10"), None, None),
            Err(Refusal::BadRequest(_))
        ));
        assert!(matches!(
            cost(json!("10"), Some("ZZZ"), None),
            Err(Refusal::Record(RecordError::UnknownCurrency { .. }))
        ));
        assert!(matches!(
            cost(json!(true), Some("AUD"), None),
            Err(Refusal::BadRequest(_))
        ));
    }

    #[test]
    fn a_body_names_its_date_once() {
        let day = NaiveDate::from_ymd_opt(2023, 5, 3).unwrap();
        let at = Utc::now();
        assert_eq!(
            work_date(Some(day), None, "occurred").unwrap(),
            Some(WorkDate::Day(day))
        );
        assert_eq!(
            work_date(None, Some(at), "occurred").unwrap(),
            Some(WorkDate::At(at))
        );
        assert_eq!(work_date(None, None, "occurred").unwrap(), None);
        assert!(work_date(Some(day), Some(at), "occurred").is_err());
    }

    #[test]
    fn a_patch_tells_a_cleared_field_from_one_left_out() {
        let body: UpdateBody = serde_json::from_value(json!({
            "expected_version": 2,
            "vendor": null,
            "cost": null,
            "completed_on": null,
        }))
        .unwrap();
        assert_eq!(body.vendor, Some(None));
        assert!(matches!(body.cost, Some(None)));
        assert_eq!(body.completed_on, Some(None));
        assert_eq!(body.reference, None);
        assert_eq!(body.holder_uuid, None);
    }

    #[test]
    fn a_patch_names_only_what_it_changes() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert_eq!(newly(Some(Some(a)), Some(b)), Some(a));
        assert_eq!(newly(Some(Some(a)), Some(a)), None);
        assert_eq!(newly(Some(None), Some(a)), None);
        assert_eq!(newly(None::<Option<Uuid>>, None), None);
    }

    #[test]
    fn a_type_is_named_by_id_or_slug() {
        let body: CreateBody =
            serde_json::from_value(json!({ "record_type": 4, "occurred_on": "2023-05-03" }))
                .unwrap();
        assert!(matches!(body.record_type, TypeRef::Id(4)));
        let body: CreateBody =
            serde_json::from_value(json!({ "record_type": "repair", "occurred_on": "2023-05-03" }))
                .unwrap();
        assert!(matches!(body.record_type, TypeRef::Slug(ref s) if s == "repair"));
        let body: CreateBody =
            serde_json::from_value(json!({ "record_type": "7", "occurred_on": "2023-05-03" }))
                .unwrap();
        assert!(matches!(body.record_type, TypeRef::Slug(ref s) if s == "7"));
    }
}
