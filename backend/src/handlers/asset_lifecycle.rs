//! Asset lifecycle endpoints: read the transition timeline and move
//! an asset to a new status.
//!
//! Lifecycle is Nosdesk-local operational state (in-repair, on-loan,
//! retired, ...), so transitions are permitted even on externally
//! synced assets: the status and its history live outside the fields
//! Intune/Entra owns.
//!
//! A transition writes an asset record only when the body asks: `record`
//! opens one of the type the target status opens with, and
//! `complete_record` completes the open one the status it leaves opened.
//!
//! `GET /assets/statuses` lists the categories with their flags and, for
//! staff, the moves the status rules allow out of each, so a client offers
//! what the server would accept.

use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::error;
use uuid::Uuid;

use crate::errors;
use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::asset_records::{self as records_api, CostBody, Refusal};
use crate::models::{AssetDisposal, AssetLifecycleEvent, AssetStatus, SANITIZATION_METHODS};
use crate::repository::{
    asset_lifecycle::{
        self as repo, CompleteRecordInput, DisposalInput, RecordInput, TransitionError,
        TransitionInput,
    },
    assets as assets_repo,
};
use crate::services::assets::status_rules::{self, MoveRefused, RefusalCode};

#[derive(Debug, Deserialize)]
pub struct TransitionBody {
    pub to_status: String,
    pub reason: Option<String>,
    /// Ticket that captured the context for this move (e.g. the
    /// repair ticket describing the fault).
    pub ticket_id: Option<i32>,
    /// State-specific fields (repair vendor / RMA / offsite, loan
    /// recipient / due-back). Stored verbatim; defaults to `{}`.
    #[serde(default)]
    pub metadata: serde_json::Value,
    /// Disposal record: required when `to_status` is `disposed`, refused
    /// otherwise. Captured atomically with the transition for the
    /// compliance / export trail.
    pub disposal: Option<DisposalBody>,
    /// Open a record of the type the target status opens with (the repair
    /// type for `in_repair`). Left out, the move writes no record.
    #[serde(default)]
    pub record: Option<TransitionRecordBody>,
    /// Complete the open record of the type the current status opens with.
    #[serde(default)]
    pub complete_record: Option<CompleteRecordBody>,
}

/// The record a transition opens: the work starts with the move and links
/// its ticket.
#[derive(Debug, Default, Deserialize)]
pub struct TransitionRecordBody {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub performed_by_uuid: Option<Uuid>,
    /// RMA, work order or invoice number.
    #[serde(default)]
    pub reference: Option<String>,
    /// Values for the type's own fields.
    #[serde(default)]
    pub attributes: Option<serde_json::Value>,
}

/// The completion a transition out of a status writes on its open record.
#[derive(Debug, Default, Deserialize)]
pub struct CompleteRecordBody {
    /// Which open record; needed only when more than one is open.
    #[serde(default)]
    pub record_id: Option<i32>,
    #[serde(default)]
    pub outcome: Option<String>,
    /// Without a currency, the amount is in the workspace currency.
    #[serde(default)]
    pub cost: Option<CostBody>,
    /// The day the work finished, or the moment; neither is the moment of
    /// the transition.
    #[serde(default)]
    pub completed_on: Option<NaiveDate>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
}

/// Why a transition is refused: by the status rules, or by the record it
/// writes.
enum Refused {
    Transition(TransitionError),
    Record(Refusal),
}

impl From<diesel::result::Error> for Refused {
    fn from(e: diesel::result::Error) -> Self {
        Refused::Transition(e.into())
    }
}

impl From<TransitionError> for Refused {
    fn from(e: TransitionError) -> Self {
        Refused::Transition(e)
    }
}

impl From<Refusal> for Refused {
    fn from(e: Refusal) -> Self {
        Refused::Record(e)
    }
}

/// Disposal detail submitted with a transition to `disposed`. Maps to the
/// repository `DisposalInput`.
#[derive(Debug, Deserialize)]
pub struct DisposalBody {
    /// NIST SP 800-88 category: clear | purge | destroy | none.
    pub sanitization_method: String,
    pub data_bearing: bool,
    pub certificate_file_id: Option<i32>,
    pub itad_vendor: Option<String>,
    pub notes: Option<String>,
}

/// Lifecycle events as anyone but staff may see them: what the status was,
/// what it became and when. The reason, who made the change, the ticket and
/// the free-form metadata (repair vendor, RMA, loan details) stay with staff,
/// as a loan's notes and actors do (`strip_staff_loan_fields`).
pub(crate) fn strip_staff_lifecycle_fields<'a>(
    events: impl IntoIterator<Item = &'a mut AssetLifecycleEvent>,
    auth: &AuthContext,
) {
    if auth.can_handle_tickets() {
        return;
    }
    for event in events {
        event.reason = None;
        event.actor_uuid = None;
        event.ticket_id = None;
        event.metadata = json!({});
    }
}

/// A disposal record as anyone but staff may see it: that the asset was
/// disposed, when, how it was sanitised and whether it held data, as the
/// asset page shows them. The notes, the ITAD vendor, the certificate and who
/// recorded it stay with staff.
pub(crate) fn strip_staff_disposal_fields(disposal: &mut AssetDisposal, auth: &AuthContext) {
    if auth.can_handle_tickets() {
        return;
    }
    disposal.notes = None;
    disposal.itad_vendor = None;
    disposal.certificate_file_id = None;
    disposal.actor_uuid = None;
}

/// An asset's lifecycle timeline. Staff get every event whole; anyone else
/// gets them through [`strip_staff_lifecycle_fields`].
pub async fn list_for_asset(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
) -> impl Responder {
    let asset_id = path.into_inner();
    match tc.run(|conn| {
        crate::handlers::assets::require_assets_visible(conn, &auth)?;
        assets_repo::get_device_by_id(conn, asset_id)?;
        repo::list_for_asset(conn, asset_id)
    }) {
        Ok(mut rows) => {
            strip_staff_lifecycle_fields(&mut rows, &auth);
            HttpResponse::Ok().json(rows)
        }
        Err(diesel::result::Error::NotFound) => {
            errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        Err(e) => {
            error!(asset_id, error = ?e, "failed to list asset lifecycle events");
            errors::internal("Failed to load asset lifecycle")
        }
    }
}

pub async fn create_transition(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    body: web::Json<TransitionBody>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can change asset status",
        );
    }

    let asset_id = path.into_inner();
    let body = body.into_inner();

    if !AssetStatus::is_valid(&body.to_status) {
        return errors::bad_request(format!("Unknown asset status '{}'", body.to_status));
    }

    // A move that writes a record names it in the metadata, which needs an
    // object to hold it.
    let writes_record = body.record.is_some() || body.complete_record.is_some();
    let metadata = if body.metadata.is_null() || (writes_record && !body.metadata.is_object()) {
        json!({})
    } else {
        body.metadata
    };
    let actor_uuid = Some(auth.user_uuid);

    // The status rules (terminal `disposed`, loan-owned `on_loan`, the
    // disposal record) and the record rules are checked by the repository
    // inside the transaction.
    let result = tc.run_result(|conn| -> Result<_, Refused> {
        let record = match body.record {
            Some(r) => {
                // The people and ticket a record names, as a record written
                // on its own checks them.
                records_api::check_references(
                    conn,
                    &auth,
                    r.performed_by_uuid,
                    None,
                    body.ticket_id,
                )?;
                Some(RecordInput {
                    summary: r.summary,
                    notes: r.notes,
                    vendor: r.vendor,
                    performed_by_uuid: r.performed_by_uuid,
                    reference: r.reference,
                    attributes: r
                        .attributes
                        .filter(|a| !a.is_null())
                        .unwrap_or_else(|| json!({})),
                })
            }
            None => None,
        };
        let complete_record = match body.complete_record {
            Some(c) => Some(CompleteRecordInput {
                record_id: c.record_id,
                outcome: c.outcome,
                cost: match c.cost {
                    Some(cost) => Some(records_api::read_cost(
                        cost,
                        records_api::workspace_currency(conn)?,
                    )?),
                    None => None,
                },
                completed: records_api::work_date(c.completed_on, c.completed_at, "completed")?,
            }),
            None => None,
        };
        Ok(repo::transition(
            conn,
            TransitionInput {
                asset_id,
                to_status: body.to_status.clone(),
                reason: body.reason.clone(),
                ticket_id: body.ticket_id,
                metadata,
                actor_uuid,
                disposal: body.disposal.as_ref().map(|d| DisposalInput {
                    sanitization_method: d.sanitization_method.clone(),
                    data_bearing: d.data_bearing,
                    certificate_file_id: d.certificate_file_id,
                    itad_vendor: d.itad_vendor.clone(),
                    notes: d.notes.clone(),
                }),
                record,
                complete_record,
            },
        )?)
    });

    match result {
        Ok((_asset, event)) => HttpResponse::Created().json(event),
        Err(Refused::Transition(e)) => map_transition_err(asset_id, e),
        Err(Refused::Record(e)) => records_api::respond(e),
    }
}

pub(crate) fn map_transition_err(asset_id: i32, e: TransitionError) -> HttpResponse {
    match e {
        TransitionError::AssetNotFound => {
            errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        TransitionError::Refused(r) => refusal_response(r),
        TransitionError::UnknownStatus(status) => {
            errors::bad_request(format!("Unknown asset status '{status}'"))
        }
        TransitionError::InvalidSanitizationMethod(method) => errors::bad_request(format!(
            "Unknown sanitization method '{method}'; expected one of: {}",
            SANITIZATION_METHODS.join(", ")
        )),
        TransitionError::NoRecordType { status } => errors::with_fields(
            StatusCode::UNPROCESSABLE_ENTITY,
            "NO_RECORD_TYPE_FOR_STATUS",
            format!("No record type opens with the status {status}"),
            json!({ "status": status }),
        ),
        TransitionError::NoOpenRecord => errors::with_fields(
            StatusCode::CONFLICT,
            "NO_OPEN_RECORD",
            "The asset has no open record to complete",
            json!({}),
        ),
        TransitionError::RecordChoiceRequired { open } => errors::with_fields(
            StatusCode::CONFLICT,
            "RECORD_CHOICE_REQUIRED",
            "More than one record is open; name the one to complete",
            json!({ "open_record_ids": open }),
        ),
        TransitionError::RecordNotOpen(id) => errors::with_fields(
            StatusCode::CONFLICT,
            "RECORD_NOT_OPEN",
            format!("Record {id} isn't an open record of this asset"),
            json!({ "record_id": id }),
        ),
        TransitionError::Record(e) => records_api::record_error(e),
        TransitionError::Db(err) => {
            error!(asset_id, error = ?err, "failed to record asset lifecycle transition");
            errors::internal("Failed to change asset status")
        }
    }
}

/// A refused move: its HTTP status (400 for a body that can't be right, 409
/// for the asset's state, 422 for a move no asset may make), its message and
/// its stable `code`.
pub(crate) fn refusal_response(r: MoveRefused) -> HttpResponse {
    let status = match r.code {
        RefusalCode::SameStatus
        | RefusalCode::NeedsDisposal
        | RefusalCode::DisposalNotAllowed
        | RefusalCode::StartOnLoan => StatusCode::BAD_REQUEST,
        RefusalCode::Terminal
        | RefusalCode::LoanOwned
        | RefusalCode::NeedsActiveLoan
        | RefusalCode::NotLoanable
        | RefusalCode::NotOnLoan => StatusCode::CONFLICT,
        RefusalCode::StartOnly => StatusCode::UNPROCESSABLE_ENTITY,
    };
    errors::with_fields(
        status,
        r.code.as_str(),
        r.message(),
        json!({ "message_key": r.code.message_key() }),
    )
}

/// One category as `GET /assets/statuses` lists it.
#[derive(Debug, Serialize)]
struct CategoryView {
    key: AssetStatus,
    /// The status a new asset gets when the create names none.
    default: bool,
    #[serde(flatten)]
    flags: status_rules::CategoryFlags,
    /// The moves by hand out of this category; staff only.
    #[serde(skip_serializing_if = "Option::is_none")]
    targets: Option<Vec<status_rules::Target>>,
}

/// GET the status categories, in picker order, with their flags and, for
/// staff, the manual targets of each and what each needs (`disposal`,
/// `active_loan`, `no_active_loan`). `statuses` lists the workspace's named
/// statuses; none exist yet. Open to whoever sees assets.
pub async fn list_statuses(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    match tc.run(|conn| crate::handlers::assets::require_assets_visible(conn, &auth)) {
        Ok(()) => {}
        Err(diesel::result::Error::NotFound) => return errors::not_found_msg("Not found"),
        Err(e) => {
            error!(error = ?e, "failed to check asset visibility");
            return errors::internal("Failed to load asset statuses");
        }
    }
    let staff = auth.can_handle_tickets();
    let categories: Vec<CategoryView> = AssetStatus::ALL
        .into_iter()
        .map(|key| CategoryView {
            key,
            default: key.as_str() == AssetStatus::default_str(),
            flags: status_rules::flags(key),
            targets: staff.then(|| status_rules::manual_targets(key)),
        })
        .collect();
    HttpResponse::Ok().json(json!({ "categories": categories, "statuses": [] }))
}

/// GET the disposal record for an asset (`None` -> 404). Read on demand for the
/// asset detail; disposal detail is not synced. Anyone but staff gets it
/// through [`strip_staff_disposal_fields`].
pub async fn get_disposal(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
) -> impl Responder {
    let asset_id = path.into_inner();
    match tc.run(|conn| {
        crate::handlers::assets::require_assets_visible(conn, &auth)?;
        assets_repo::get_device_by_id(conn, asset_id)?;
        repo::disposal_for_asset(conn, asset_id)
    }) {
        Ok(Some(mut d)) => {
            strip_staff_disposal_fields(&mut d, &auth);
            HttpResponse::Ok().json(d)
        }
        Ok(None) => errors::not_found_msg(format!("Asset {asset_id} has no disposal record")),
        Err(diesel::result::Error::NotFound) => {
            errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        Err(e) => {
            error!(asset_id, error = ?e, "failed to load asset disposal");
            errors::internal("Failed to load asset disposal")
        }
    }
}

/// GET distinct ITAD vendors previously entered, for the disposal-form
/// suggestions. Staff only, as the disposal form and a record's vendor are:
/// anyone else gets an empty list.
pub async fn list_itad_vendors(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    match tc.run(|conn| {
        if !auth.can_handle_tickets() {
            return Ok(Vec::new());
        }
        repo::list_itad_vendors(conn)
    }) {
        Ok(vendors) => HttpResponse::Ok().json(vendors),
        Err(e) => {
            error!(error = ?e, "failed to list ITAD vendors");
            errors::internal("Failed to load ITAD vendors")
        }
    }
}
