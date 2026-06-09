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
use crate::models::AssetStatus;
use crate::repository::{
    asset_lifecycle::{self as repo, TransitionInput},
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
            return Ok(None);
        }
        repo::transition(
            conn,
            TransitionInput {
                asset_id,
                to_status: body.to_status.clone(),
                reason: body.reason.clone(),
                ticket_id: body.ticket_id,
                metadata: metadata.clone(),
                actor_uuid,
            },
        )
        .map(Some)
    });

    match result {
        Ok(Some((_asset, event))) => HttpResponse::Created().json(event),
        Ok(None) => errors::bad_request("Asset is already in the requested status"),
        Err(diesel::result::Error::NotFound) => {
            errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        Err(e) => {
            error!(asset_id, error = ?e, "failed to record asset lifecycle transition");
            errors::internal("Failed to change asset status")
        }
    }
}
