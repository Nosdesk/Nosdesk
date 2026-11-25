//! The asset timeline over REST: `GET /api/assets/{id}/timeline`.
//!
//! One page of what happened to the asset, newest first, merged from its
//! status changes (with the disposal folded in), records, replacements,
//! loans, linked tickets and markers. Query parameters:
//!
//! - `sources`: a comma-separated subset of `status`, `record`,
//!   `replacement`, `loan`, `ticket`, `marker`; all when absent.
//! - `record_type`: records (and replacements) of this type only, by slug or
//!   id.
//! - `from`, `to`: entries on or after, on or before these days.
//! - `before`: the `next_cursor` of the page before.
//! - `limit`: 1 to 200, 100 by default.
//! - `include_predecessors`: also the timelines of the assets this one
//!   replaced, each entry naming its asset.
//!
//! Each entry carries the row as its own endpoint gives it to the viewer.
//! Anyone shown assets reads the timeline; records, replacements and
//! predecessors are staff only, tickets are the ones the viewer can see, and
//! anyone but staff gets status changes, loans and disposals without their
//! staff fields. The record card and the timeline export read entries
//! through [`project`] too.

use std::collections::{HashMap, HashSet};

use actix_web::http::Method;
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tracing::error;
use uuid::Uuid;

use crate::errors;
use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::asset_lifecycle::{strip_staff_disposal_fields, strip_staff_lifecycle_fields};
use crate::handlers::asset_loans::strip_staff_loan_fields;
use crate::middleware::token_scope::claims_may_call;
use crate::models::{Asset, Claims};
use crate::repository::asset_timeline::{
    self as repo, Entry, EntryData, EntryKey, Source, TimelineFilter, Viewer,
};
use crate::repository::ticket_visibility::VisibilityContext;
use crate::repository::{asset_record_types as types_repo, assets as assets_repo};
use crate::services::imports::assets::TimelineExportRow;

const PAGE_DEFAULT: i64 = 100;
const PAGE_MAX: i64 = 200;

#[derive(Debug, Deserialize)]
pub struct TimelineQuery {
    pub sources: Option<String>,
    pub record_type: Option<String>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub before: Option<String>,
    pub limit: Option<i64>,
    #[serde(default)]
    pub include_predecessors: bool,
}

/// The viewer a request reads the timeline as.
pub(crate) fn viewer(auth: &AuthContext) -> Viewer {
    Viewer {
        staff: auth.can_handle_tickets(),
        tickets: VisibilityContext::from_auth(auth),
    }
}

/// The sources a comma-separated list names; all when absent or blank.
pub(crate) fn parse_sources(raw: Option<&str>) -> Result<HashSet<Source>, String> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Ok(Source::ALL.into_iter().collect());
    };
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Source::parse(s).ok_or_else(|| format!("unknown timeline source '{s}'")))
        .collect()
}

/// `from` and `to` as days the timeline can read, else why not.
pub(crate) fn check_window(from: Option<NaiveDate>, to: Option<NaiveDate>) -> Result<(), String> {
    if from.into_iter().chain(to).all(repo::day_in_range) {
        Ok(())
    } else {
        Err(format!(
            "from and to must fall between {} and {}",
            repo::FIRST_DAY,
            repo::LAST_DAY
        ))
    }
}

/// What a `record_type` parameter narrows records to.
pub(crate) enum TypeFilter {
    Any,
    Only(crate::models::AssetRecordType),
    /// No type by that slug or id: no records.
    Unknown,
}

/// The record type a parameter names, by slug, else by id.
pub(crate) fn type_filter(
    conn: &mut crate::db::DbConnection,
    raw: Option<&str>,
) -> diesel::QueryResult<TypeFilter> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Ok(TypeFilter::Any);
    };
    let found = match types_repo::get_by_slug(conn, raw)? {
        Some(t) => Some(t),
        None => match raw.parse::<i32>() {
            Ok(id) => types_repo::get(conn, id)?,
            Err(_) => None,
        },
    };
    Ok(found.map_or(TypeFilter::Unknown, TypeFilter::Only))
}

/// The timeline filter a request's parameters describe.
pub(crate) fn filter_for(
    sources: HashSet<Source>,
    types: &TypeFilter,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> TimelineFilter {
    let mut filter = TimelineFilter {
        sources,
        record_type_id: None,
        from,
        to,
    };
    match types {
        TypeFilter::Any => {}
        TypeFilter::Only(t) => filter.record_type_id = Some(t.id),
        TypeFilter::Unknown => {
            filter.sources.remove(&Source::Record);
            filter.sources.remove(&Source::Replacement);
        }
    }
    filter
}

/// An entry as the viewer may see it.
#[derive(Debug, Clone)]
pub(crate) struct Projected {
    pub key: EntryKey,
    pub source: Source,
    /// Which of a source's entries this is, where it has more than one kind:
    /// `lent` or `returned`, `replaces` or `replaced_by`, `purchased` or
    /// `added`.
    pub kind: Option<&'static str>,
    /// Who did it: who moved the status, did the work, lent or took back the
    /// asset.
    pub actor_uuid: Option<Uuid>,
    /// Who wrote it down.
    pub recorded_by: Option<Uuid>,
    /// Whether the entry shows when it happened and was written down, or
    /// only its day: a ticket keeps its times from anyone but staff whose
    /// credential reads tickets.
    pub timed: bool,
    pub data: Value,
}

impl Projected {
    /// When it happened, unless the entry shows only its day.
    pub fn at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.key.at.filter(|_| self.timed)
    }

    /// When it was written down, unless the entry shows only its day.
    pub fn recorded_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.timed.then_some(self.key.recorded_at)
    }

    /// A key unique within a timeline.
    pub fn key_text(&self) -> String {
        let k = &self.key;
        match self.source {
            Source::Ticket => format!("ticket:{}@{}", k.id, k.asset_id),
            Source::Marker => format!("marker:{}@{}", self.kind.unwrap_or(""), k.asset_id),
            _ => match self.kind {
                Some(kind) => format!("{}:{}:{kind}", self.source.as_str(), k.id),
                None => format!("{}:{}", self.source.as_str(), k.id),
            },
        }
    }

    pub fn to_json(&self) -> Value {
        let mut out = json!({
            "key": self.key_text(),
            "source": self.source.as_str(),
            "asset_id": self.key.asset_id,
            "day": self.key.day,
            "at": self.at(),
            "recorded_at": self.recorded_at(),
            "recorded_by": self.recorded_by,
            "actor_uuid": self.actor_uuid,
            "data": self.data,
        });
        if let (Some(kind), Value::Object(map)) = (self.kind, &mut out) {
            map.insert("kind".into(), json!(kind));
        }
        out
    }
}

fn to_value<T: serde::Serialize>(row: &T) -> Value {
    serde_json::to_value(row).unwrap_or(Value::Null)
}

/// Whether the request's credential may read tickets. Sessions and
/// unnarrowed tokens may; an API token narrowed to other scopes (such as
/// `assets:read`) may not, so the timeline names its tickets by id alone.
pub(crate) fn reads_tickets(req: &HttpRequest) -> bool {
    req.extensions()
        .get::<Claims>()
        .is_some_and(|c| claims_may_call(c, &Method::GET, "/api/tickets"))
}

/// Entries as `auth` may see them: records as the record endpoint answers
/// them, status changes, disposals and loans through their strip functions,
/// tickets without their dates for anyone but staff, and by id alone when
/// the credential can't read tickets (`ticket_details` false). A ticket
/// without its dates keeps its day but not its time or entry time. Records
/// and replacements never reach anyone but staff.
pub(crate) fn project(
    conn: &mut crate::db::DbConnection,
    auth: &AuthContext,
    entries: Vec<Entry>,
    ticket_details: bool,
) -> diesel::QueryResult<Vec<Projected>> {
    let staff = auth.can_handle_tickets();
    let records: Vec<crate::models::AssetRecord> = entries
        .iter()
        .filter_map(|e| match &e.data {
            EntryData::Record(r) if staff => Some(r.clone()),
            _ => None,
        })
        .collect();
    let record_json: HashMap<i64, Value> = if records.is_empty() {
        HashMap::new()
    } else {
        crate::handlers::asset_records::records_json(conn, &records)?
            .into_iter()
            .filter_map(|v| v["id"].as_i64().map(|id| (id, v)))
            .collect()
    };
    let slugs: HashMap<i32, String> = if staff
        && entries
            .iter()
            .any(|e| matches!(e.data, EntryData::Replacement { .. }))
    {
        types_repo::list(conn, true)?
            .into_iter()
            .map(|t| (t.id, t.slug))
            .collect()
    } else {
        HashMap::new()
    };

    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let key = entry.key;
        let source = entry.data.source();
        let projected = match entry.data {
            EntryData::Status {
                mut event,
                disposal,
                record_ids,
            } => {
                strip_staff_lifecycle_fields(std::iter::once(&mut event), auth);
                let disposal = disposal.map(|mut d| {
                    strip_staff_disposal_fields(&mut d, auth);
                    d
                });
                let mut data = to_value(&event);
                if let Value::Object(map) = &mut data {
                    map.insert("disposal".into(), to_value(&disposal));
                    if staff {
                        map.insert("record_ids".into(), json!(record_ids));
                    }
                }
                Projected {
                    key,
                    source,
                    kind: None,
                    timed: true,
                    actor_uuid: event.actor_uuid,
                    recorded_by: event.actor_uuid,
                    data,
                }
            }
            EntryData::Record(record) => {
                if !staff {
                    continue;
                }
                Projected {
                    key,
                    source,
                    kind: None,
                    timed: true,
                    actor_uuid: record.performed_by_uuid,
                    recorded_by: record.recorded_by,
                    data: record_json
                        .get(&i64::from(record.id))
                        .cloned()
                        .unwrap_or(Value::Null),
                }
            }
            EntryData::Replacement {
                record,
                replaces,
                other,
            } => {
                if !staff {
                    continue;
                }
                Projected {
                    key,
                    source,
                    kind: Some(if replaces { "replaces" } else { "replaced_by" }),
                    timed: true,
                    actor_uuid: None,
                    recorded_by: record.recorded_by,
                    data: json!({
                        "record_id": record.id,
                        "record_type": slugs.get(&record.record_type_id),
                        "outcome": record.outcome,
                        "other_asset": other.map(|a| json!({
                            "id": a.id,
                            "name": a.name,
                            "asset_tag": a.asset_tag,
                        })),
                    }),
                }
            }
            EntryData::LoanOut(mut loan) | EntryData::LoanReturned(mut loan) => {
                let returned = key.rank == repo::Rank::LoanReturned;
                strip_staff_loan_fields(std::iter::once(&mut loan), auth);
                let actor = if returned {
                    loan.returned_by_uuid
                } else {
                    loan.actor_uuid
                };
                Projected {
                    key,
                    source,
                    kind: Some(if returned { "returned" } else { "lent" }),
                    timed: true,
                    actor_uuid: actor,
                    recorded_by: actor,
                    data: to_value(&loan),
                }
            }
            EntryData::Ticket(t) => {
                let state = t.state.map(|s| {
                    json!({
                        "id": s.id,
                        "name": s.name,
                        "category": s.category,
                    })
                });
                let data = if !ticket_details {
                    json!({ "id": t.id })
                } else if staff {
                    json!({
                        "id": t.id,
                        "number": t.number,
                        "title": t.title,
                        "state": state,
                        "created_at": t.created_at,
                        "closed_at": t.closed_at,
                        "linked_at": t.linked_at,
                    })
                } else {
                    json!({
                        "id": t.id,
                        "number": t.number,
                        "title": t.title,
                        "state": state,
                    })
                };
                Projected {
                    key,
                    source,
                    kind: None,
                    // The day orders the entry; the times are the ticket's
                    // own, which only staff who read tickets see.
                    timed: staff && ticket_details,
                    actor_uuid: None,
                    recorded_by: None,
                    data,
                }
            }
            EntryData::Marker(marker) => Projected {
                key,
                source,
                kind: Some(marker.as_str()),
                timed: true,
                actor_uuid: None,
                recorded_by: None,
                data: json!({}),
            },
        };
        out.push(projected);
    }
    Ok(out)
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

/// Timeline entries as export rows, each with its asset's tag and name. A
/// status change's details name its named statuses from `status_labels`
/// (`from_status_label`, `to_status_label`), so a change within one
/// category reads as more than `in_repair` to `in_repair`.
pub(crate) fn export_rows(
    entries: &[Projected],
    assets: &HashMap<i32, &Asset>,
    status_labels: &HashMap<i32, String>,
) -> Vec<TimelineExportRow> {
    entries
        .iter()
        .map(|e| {
            let d = &e.data;
            let asset = assets.get(&e.key.asset_id);
            let mut row = TimelineExportRow {
                asset_tag: asset.and_then(|a| a.asset_tag.clone()),
                asset_name: asset.map(|a| a.name.clone()).unwrap_or_default(),
                source: e.source.as_str(),
                day: e.key.day,
                at: e.at(),
                kind: e.kind.map(str::to_string),
                summary: None,
                outcome: None,
                vendor: None,
                reference: None,
                cost: None,
                currency: None,
                ticket_id: None,
                actor_uuid: e.actor_uuid,
                recorded_at: e.recorded_at(),
                recorded_by: e.recorded_by,
                details: d.clone(),
            };
            match e.source {
                Source::Record => {
                    row.kind = text(&d["record_type"]);
                    row.summary = text(&d["summary"]);
                    row.outcome = text(&d["outcome"]);
                    row.vendor = text(&d["vendor"]);
                    row.reference = text(&d["reference"]);
                    row.cost = text(&d["cost"]["amount"]);
                    row.currency = text(&d["cost"]["currency"]);
                    row.ticket_id = d["ticket_id"].as_i64().map(|t| t as i32);
                }
                Source::Status => {
                    row.kind = text(&d["to_status"]);
                    row.summary = text(&d["reason"]);
                    row.ticket_id = d["ticket_id"].as_i64().map(|t| t as i32);
                    if let Value::Object(details) = &mut row.details {
                        for side in ["from", "to"] {
                            let label = d[format!("{side}_status_id")]
                                .as_i64()
                                .and_then(|id| status_labels.get(&(id as i32)));
                            if let Some(label) = label {
                                details.insert(format!("{side}_status_label"), json!(label));
                            }
                        }
                    }
                }
                Source::Replacement => {
                    row.summary = text(&d["other_asset"]["asset_tag"])
                        .or_else(|| text(&d["other_asset"]["name"]));
                    row.outcome = text(&d["outcome"]);
                }
                Source::Loan => {
                    row.ticket_id = d["ticket_id"].as_i64().map(|t| t as i32);
                }
                Source::Ticket => {
                    row.kind = text(&d["state"]["name"]);
                    row.summary = text(&d["title"]);
                    row.ticket_id = d["id"].as_i64().map(|t| t as i32);
                }
                Source::Marker => {}
            }
            row
        })
        .collect()
}

/// A JSON array of export rows, objects keyed by column.
pub(crate) fn export_rows_json(rows: &[TimelineExportRow]) -> Vec<Map<String, Value>> {
    rows.iter()
        .filter_map(|r| match serde_json::to_value(r) {
            Ok(Value::Object(map)) => Some(map),
            _ => None,
        })
        .collect()
}

/// Why a timeline request failed.
enum Refusal {
    BadRequest(String),
    AssetNotFound,
    Db(diesel::result::Error),
}

impl From<diesel::result::Error> for Refusal {
    fn from(e: diesel::result::Error) -> Self {
        match e {
            diesel::result::Error::NotFound => Refusal::AssetNotFound,
            other => Refusal::Db(other),
        }
    }
}

/// `GET /api/assets/{id}/timeline`.
pub async fn list_for_asset(
    mut tc: TenantConn,
    auth: AuthContext,
    req: HttpRequest,
    path: web::Path<i32>,
    query: web::Query<TimelineQuery>,
) -> HttpResponse {
    let asset_id = path.into_inner();
    let query = query.into_inner();
    let ticket_details = reads_tickets(&req);
    let result = tc.run_result(|conn| {
        let sources = parse_sources(query.sources.as_deref()).map_err(Refusal::BadRequest)?;
        let after = match query.before.as_deref().filter(|c| !c.is_empty()) {
            Some(raw) => Some(EntryKey::decode(raw).ok_or_else(|| {
                Refusal::BadRequest("before isn't a cursor this timeline gave".into())
            })?),
            None => None,
        };
        check_window(query.from, query.to).map_err(Refusal::BadRequest)?;
        let limit = query.limit.unwrap_or(PAGE_DEFAULT).clamp(1, PAGE_MAX);
        crate::handlers::assets::require_assets_visible(conn, &auth)?;
        assets_repo::get_device_by_id(conn, asset_id)?;
        let viewer = viewer(&auth);
        // Only staff see records, so only theirs read the type.
        let types = if viewer.staff {
            type_filter(conn, query.record_type.as_deref())?
        } else {
            TypeFilter::Any
        };
        let filter = filter_for(sources, &types, query.from, query.to);
        let mut asset_ids = vec![asset_id];
        // The chain is read from records, which only staff see.
        if query.include_predecessors && viewer.staff {
            asset_ids.extend(repo::predecessors(conn, asset_id)?);
        }
        let page = repo::page(conn, &asset_ids, &viewer, &filter, after, Some(limit))?;
        let entries: Vec<Value> = project(conn, &auth, page.entries, ticket_details)?
            .iter()
            .map(Projected::to_json)
            .collect();
        Ok(json!({
            "entries": entries,
            "next_cursor": page.next.as_ref().map(EntryKey::encode),
        }))
    });
    match result {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(Refusal::BadRequest(message)) => errors::bad_request(message),
        Err(Refusal::AssetNotFound) => errors::not_found_msg(format!("Asset {asset_id} not found")),
        Err(Refusal::Db(e)) => {
            error!(asset_id, error = ?e, "failed to read the asset timeline");
            errors::internal("Failed to load the asset timeline")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_read_from_a_comma_separated_list() {
        assert_eq!(parse_sources(None).unwrap().len(), Source::ALL.len());
        assert_eq!(parse_sources(Some(" ")).unwrap().len(), Source::ALL.len());
        let some = parse_sources(Some("record, status,")).unwrap();
        assert_eq!(some, HashSet::from([Source::Record, Source::Status]));
        assert!(parse_sources(Some("record,history")).is_err());
    }

    #[test]
    fn an_unknown_type_leaves_records_out() {
        let filter = filter_for(
            Source::ALL.into_iter().collect(),
            &TypeFilter::Unknown,
            None,
            None,
        );
        assert!(!filter.sources.contains(&Source::Record));
        assert!(!filter.sources.contains(&Source::Replacement));
        assert!(filter.sources.contains(&Source::Status));
    }
}
