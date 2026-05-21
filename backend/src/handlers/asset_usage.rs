//! Asset usage endpoints.
//!
//! - `POST /api/assets/{id}/usage` — record a usage event.
//!   Technician + admin only. Refused for assets that aren't
//!   stock-tracked (assets.quantity IS NULL).
//! - `GET  /api/assets/{id}/usage` — paginated history for an
//!   asset.
//! - `GET  /api/tickets/{id}/asset-usage` — usage rows attached
//!   to a ticket.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use bigdecimal::BigDecimal;
use diesel::result::Error as DieselError;
use serde::Deserialize;
use std::str::FromStr;
use tracing::error;

use crate::db::Pool;
use crate::handlers::sse::{SseEvent, SseState};
use crate::handlers::{errors, helpers};
use crate::models::NewAssetUsage;
use crate::repository::asset_usage as repo;
use crate::utils::rbac::is_technician_or_admin;

#[derive(Debug, Deserialize)]
pub struct RecordUsageBody {
    /// Decimal string. Diesel's BigDecimal can't safely round-
    /// trip through f64 for quantities measured to 3dp, so the
    /// wire format is text.
    pub quantity_used: String,
    /// Optional tie-in to a ticket. If absent, the row is an
    /// ad-hoc consumption (restock audit, write-off).
    #[serde(default)]
    pub ticket_id: Option<i32>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListUsageQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

pub async fn record(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<RecordUsageBody>,
    sse_state: web::Data<SseState>,
) -> impl Responder {
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(c) => c.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };
    if !is_technician_or_admin(&claims) {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can record usage",
        );
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let asset_id = path.into_inner();
    let body = body.into_inner();

    // Parse and bounds-check the quantity. Decimal-string in,
    // BigDecimal out; reject zero / negative immediately because
    // the CHECK constraint on the DB would also catch them but
    // a 422 with a clear message is friendlier than a 500.
    let quantity = match BigDecimal::from_str(body.quantity_used.trim()) {
        Ok(q) => q,
        Err(_) => return errors::bad_request("quantity_used must be a decimal number"),
    };
    if quantity <= BigDecimal::from(0) {
        return errors::bad_request("quantity_used must be greater than zero");
    }

    // Look up the asset's current quantity + unit. Refuse usage
    // writes for assets that aren't stock-tracked (quantity is
    // NULL); the unit lives on the asset's own row so we can
    // stamp it onto the usage row without trusting client input.
    use crate::schema::assets;
    use diesel::prelude::*;
    let asset_row: Result<(Option<BigDecimal>, Option<String>), DieselError> = assets::table
        .find(asset_id)
        .select((assets::quantity, assets::unit))
        .first(&mut conn);
    let (current_qty, unit) = match asset_row {
        Ok(row) => row,
        Err(DieselError::NotFound) => {
            return errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        Err(e) => {
            error!(asset_id, error = ?e, "failed to load asset for usage record");
            return errors::internal("Failed to load asset");
        }
    };
    if current_qty.is_none() {
        return errors::unprocessable_entity(
            "Asset is not stock-tracked: set a quantity on the asset before recording usage",
        );
    }
    let unit = match unit {
        Some(u) if !u.is_empty() => u,
        _ => {
            return errors::unprocessable_entity(
                "Asset has no unit; set one before recording usage",
            )
        }
    };

    let recorded_by = uuid::Uuid::parse_str(&claims.sub).ok();
    let new_usage = NewAssetUsage {
        asset_id,
        ticket_id: body.ticket_id,
        quantity_used: quantity,
        unit: unit.clone(),
        recorded_by,
        notes: body.notes.and_then(|s| {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }),
    };

    match repo::record_usage(&mut conn, new_usage) {
        Ok(outcome) => {
            if outcome.crossed_low_stock {
                if let Some(threshold) = outcome.threshold.as_ref() {
                    sse_state
                        .broadcast_event(SseEvent::AssetLowStock {
                            device_id: asset_id,
                            device_name: outcome.asset_name.clone(),
                            quantity: outcome.new_quantity.to_string(),
                            threshold: threshold.to_string(),
                            unit: unit.clone(),
                            timestamp: chrono::Utc::now(),
                        })
                        .await;
                }
            }
            HttpResponse::Created().json(outcome.row)
        }
        Err(DieselError::DatabaseError(
            diesel::result::DatabaseErrorKind::ForeignKeyViolation,
            _,
        )) => {
            // Most likely cause: ticket_id pointed at a deleted ticket.
            errors::bad_request("Referenced ticket does not exist")
        }
        Err(e) => {
            error!(asset_id, error = ?e, "failed to record asset usage");
            errors::internal("Failed to record usage")
        }
    }
}

pub async fn list_for_asset(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    query: web::Query<ListUsageQuery>,
) -> impl Responder {
    let (_claims, _user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let asset_id = path.into_inner();
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);

    match repo::list_for_asset(&mut conn, asset_id, limit, offset) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(asset_id, error = ?e, "failed to list asset usage");
            errors::internal("Failed to load usage history")
        }
    }
}

pub async fn list_for_ticket(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    let (_claims, _user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let ticket_id = path.into_inner();
    match repo::list_for_ticket(&mut conn, ticket_id) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(ticket_id, error = ?e, "failed to list ticket usage");
            errors::internal("Failed to load usage history")
        }
    }
}
