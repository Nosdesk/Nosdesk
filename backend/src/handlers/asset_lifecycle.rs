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

use crate::errors;
use crate::extractors::{AuthContext, TenantConn};
use crate::models::{AssetStatus, SANITIZATION_METHODS};
use crate::repository::{
    asset_lifecycle::{self as repo, DisposalInput, TransitionError, TransitionInput},
    assets as assets_repo,
};

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

    // The status rules (terminal `disposed`, loan-owned `on_loan`, the
    // disposal record) are checked by the repository inside the transaction.
    let result = tc.run_result(|conn| {
        repo::transition(
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
            },
        )
    });

    match result {
        Ok((_asset, event)) => HttpResponse::Created().json(event),
        Err(e) => map_transition_err(asset_id, e),
    }
}

fn map_transition_err(asset_id: i32, e: TransitionError) -> HttpResponse {
    match e {
        TransitionError::AssetNotFound => {
            errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        TransitionError::NoOp => errors::bad_request("Asset is already in the requested status"),
        TransitionError::FromDisposed => {
            errors::conflict("Disposed assets can't change status; disposal is final")
        }
        TransitionError::OnLoanReturnViaLoan => {
            errors::conflict("Asset is out on loan; return the loan to change its status")
        }
        TransitionError::OnLoanNeedsActiveLoan => errors::conflict(
            "On-loan status is set by issuing a loan; there is no active loan to restore",
        ),
        TransitionError::DisposalRequired => {
            errors::bad_request("Disposing an asset requires a disposal record")
        }
        TransitionError::DisposalNotAllowed => {
            errors::bad_request("A disposal record is only accepted when disposing an asset")
        }
        TransitionError::InvalidSanitizationMethod(method) => errors::bad_request(format!(
            "Unknown sanitization method '{method}'; expected one of: {}",
            SANITIZATION_METHODS.join(", ")
        )),
        TransitionError::Db(err) => {
            error!(asset_id, error = ?err, "failed to record asset lifecycle transition");
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
