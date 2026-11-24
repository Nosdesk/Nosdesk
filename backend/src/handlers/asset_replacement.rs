//! `POST /api/assets/{id}/records/{record_id}/replacement`: the carry-overs
//! from a replaced asset to the asset its record names as the replacement.
//!
//! Each carry-over is asked for on its own and runs through its usual write
//! path (see [`crate::repository::asset_replacement`]); all of them run in
//! one transaction. Staff only, as record writes are: anyone else gets 404.

use std::sync::Arc;

use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use crate::errors;
use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::asset_lifecycle::{map_transition_err, DisposalBody};
use crate::handlers::asset_records::{require_staff, respond};
use crate::models::AssetStatus;
use crate::repository::asset_lifecycle::DisposalInput;
use crate::repository::asset_replacement::{
    self as repo, CarryOver, MoveOld, ReplacementError, OLD_ASSET_STATUSES,
};
use crate::repository::assets as assets_repo;
use crate::repository::ticket_visibility::VisibilityContext;
use crate::services::search::{indexing_tasks, SearchService};

#[derive(Debug, Default, Deserialize)]
pub struct CarryOverBody {
    /// Link the old asset's open tickets to the new one too.
    #[serde(default)]
    pub link_open_tickets: bool,
    /// Copy holder, managed by, location and groups to the new asset.
    #[serde(default)]
    pub copy_assignment: bool,
    /// Move the old asset to `retired`, `in_stock` or `disposed`.
    #[serde(default)]
    pub move_old_to: Option<String>,
    /// The disposal record a move to `disposed` needs, as the status change
    /// takes it.
    #[serde(default)]
    pub disposal: Option<DisposalBody>,
}

pub async fn carry_over(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<(i32, i32)>,
    body: web::Json<CarryOverBody>,
    search: Option<web::Data<Arc<SearchService>>>,
) -> HttpResponse {
    if let Err(r) = require_staff(&auth) {
        return respond(r);
    }
    let (asset_id, record_id) = path.into_inner();
    let body = body.into_inner();

    let move_old_to = match body.move_old_to.as_deref().filter(|s| !s.is_empty()) {
        Some(raw) => {
            match AssetStatus::from_str_opt(raw).filter(|s| OLD_ASSET_STATUSES.contains(s)) {
                Some(status) => Some(MoveOld {
                    status,
                    disposal: body.disposal.as_ref().map(|d| DisposalInput {
                        sanitization_method: d.sanitization_method.clone(),
                        data_bearing: d.data_bearing,
                        certificate_file_id: d.certificate_file_id,
                        itad_vendor: d.itad_vendor.clone(),
                        notes: d.notes.clone(),
                    }),
                }),
                None => {
                    return errors::bad_request(format!(
                        "move_old_to must be one of: {}",
                        OLD_ASSET_STATUSES.map(AssetStatus::as_str).join(", ")
                    ))
                }
            }
        }
        None if body.disposal.is_some() => {
            return errors::bad_request(
                "A disposal record is only accepted when disposing an asset",
            )
        }
        None => None,
    };
    let input = CarryOver {
        link_open_tickets: body.link_open_tickets,
        copy_assignment: body.copy_assignment,
        move_old_to,
    };
    let viewer = VisibilityContext::from_auth(&auth);
    let actor = Some(auth.user_uuid);

    match tc.run_result(|conn| repo::carry_over(conn, asset_id, record_id, input, &viewer, actor)) {
        Ok(result) => {
            // The new asset's search entry carries its holder and location.
            let changed = result
                .assignment
                .as_ref()
                .is_some_and(|a| !a.copied.is_empty());
            if let (true, Some(search)) = (changed, search) {
                let new_id = result.replacement_asset_id;
                match tc.run(|conn| assets_repo::get_device_by_id(conn, new_id)) {
                    Ok(asset) => {
                        indexing_tasks::spawn_index_device(search.get_ref().clone(), asset)
                    }
                    Err(e) => {
                        error!(asset_id = new_id, error = ?e, "failed to read the replacement asset to index it")
                    }
                }
            }
            HttpResponse::Ok().json(result)
        }
        Err(ReplacementError::RecordNotFound) => errors::not_found_msg("Record not found"),
        Err(ReplacementError::NotReplaced) => errors::with_fields(
            StatusCode::UNPROCESSABLE_ENTITY,
            "RECORD_NOT_REPLACED",
            "The record's outcome isn't a replacement",
            json!({}),
        ),
        Err(ReplacementError::NoReplacementAsset) => errors::with_fields(
            StatusCode::UNPROCESSABLE_ENTITY,
            "NO_REPLACEMENT_ASSET",
            "The record names no replacement asset",
            json!({}),
        ),
        Err(ReplacementError::Transition(e)) => map_transition_err(asset_id, e),
        Err(ReplacementError::Db(e)) => {
            error!(asset_id, error = ?e, "failed to carry over to the replacement asset");
            errors::internal("Failed to carry over to the replacement asset")
        }
    }
}
