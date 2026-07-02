//! Asset lifecycle endpoints: read the transition timeline and move
//! an asset to a new status.
//!
//! Lifecycle is Nosdesk-local operational state (in-repair, on-loan,
//! retired, ...), so transitions are permitted even on externally
//! synced assets: the status and its history live outside the fields
//! Intune/Entra owns.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::{AssetLifecycleEvent, AssetStatus};
use crate::repository::{
    asset_lifecycle::{self as repo, DisposalInput, TransitionInput},
    asset_loans as asset_loans_repo, assets as assets_repo,
};

/// Outcome of a transition attempt: success carries the new event; the two
/// client-error cases each map to a distinct 400.
enum TransitionResult {
    Done(AssetLifecycleEvent),
    NoOp,
    OnLoanNeedsActiveLoan,
}

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
    /// Disposal record, sent only when `to_status` is `disposed`. Captured
    /// atomically with the transition for the compliance / export trail.
    pub disposal: Option<DisposalBody>,
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

pub async fn list_for_asset(
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<i32>,
) -> impl Responder {
    let asset_id = path.into_inner();
    match tc.run(|conn| {
        assets_repo::get_device_by_id(conn, asset_id)?;
        repo::list_for_asset(conn, asset_id)
    }) {
        Ok(rows) => HttpResponse::Ok().json(rows),
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

    let metadata = if body.metadata.is_null() {
        json!({})
    } else {
        body.metadata
    };
    let actor_uuid = Some(auth.user_uuid);

    // `None` signals a no-op (already in the target status); the
    // whole check + transition share one connection so they're
    // atomic.
    let result = tc.run(|conn| {
        let current = assets_repo::get_device_by_id(conn, asset_id)?;
        if current.status == body.to_status {
            return Ok(TransitionResult::NoOp);
        }
        // `on_loan` is owned by the loan ledger. Allow it as a manual target
        // only to restore an asset that still has an active loan (e.g. undoing
        // a mistaken `lost`); otherwise it must come from issuing a loan.
        if body.to_status == "on_loan"
            && asset_loans_repo::active_for_asset(conn, asset_id)?.is_none()
        {
            return Ok(TransitionResult::OnLoanNeedsActiveLoan);
        }
        let (_asset, event) = repo::transition(
            conn,
            TransitionInput {
                asset_id,
                to_status: body.to_status.clone(),
                reason: body.reason.clone(),
                ticket_id: body.ticket_id,
                metadata: metadata.clone(),
                actor_uuid,
                disposal: body.disposal.as_ref().map(|d| DisposalInput {
                    sanitization_method: d.sanitization_method.clone(),
                    data_bearing: d.data_bearing,
                    certificate_file_id: d.certificate_file_id,
                    itad_vendor: d.itad_vendor.clone(),
                    notes: d.notes.clone(),
                }),
            },
        )?;
        Ok(TransitionResult::Done(event))
    });

    match result {
        Ok(TransitionResult::Done(event)) => HttpResponse::Created().json(event),
        Ok(TransitionResult::NoOp) => {
            errors::bad_request("Asset is already in the requested status")
        }
        Ok(TransitionResult::OnLoanNeedsActiveLoan) => errors::bad_request(
            "On-loan status is set by issuing a loan; there is no active loan to restore",
        ),
        Err(diesel::result::Error::NotFound) => {
            errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        Err(e) => {
            error!(asset_id, error = ?e, "failed to record asset lifecycle transition");
            errors::internal("Failed to change asset status")
        }
    }
}

/// GET the disposal record for an asset (`None` -> 404). Read on demand for the
/// asset detail + the record-card export; disposal detail is not synced.
pub async fn get_disposal(
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<i32>,
) -> impl Responder {
    let asset_id = path.into_inner();
    match tc.run(|conn| {
        assets_repo::get_device_by_id(conn, asset_id)?;
        repo::disposal_for_asset(conn, asset_id)
    }) {
        Ok(Some(d)) => HttpResponse::Ok().json(d),
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
/// suggestions. Any authenticated workspace member.
pub async fn list_itad_vendors(mut tc: TenantConn, _auth: AuthContext) -> impl Responder {
    match tc.run(repo::list_itad_vendors) {
        Ok(vendors) => HttpResponse::Ok().json(vendors),
        Err(e) => {
            error!(error = ?e, "failed to list ITAD vendors");
            errors::internal("Failed to load ITAD vendors")
        }
    }
}
