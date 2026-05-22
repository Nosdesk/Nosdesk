//! Asset audit endpoints.
//!
//! - `POST /api/assets/{id}/audit` — record a physical-count
//!   audit. Technician + admin only. Refused for assets that
//!   aren't stock-tracked (assets.quantity IS NULL) and for
//!   externally-synced rows where manual correction would just
//!   be overwritten by the next sync.
//! - `GET  /api/assets/{id}/audits` — paginated history.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use bigdecimal::BigDecimal;
use diesel::result::Error as DieselError;
use serde::Deserialize;
use std::str::FromStr;
use tracing::error;

use crate::db::Pool;
use crate::handlers::sse::{SseEvent, SseState};
use crate::handlers::{errors, helpers};
use crate::repository::asset_audits as repo;
use crate::services::notifications::types::{
    NotificationActor, NotificationEntity, NotificationPayload, NotificationTypeCode,
};
use crate::services::notifications::NotificationService;
use crate::utils::rbac::is_technician_or_admin;

#[derive(Debug, Deserialize)]
pub struct RecordAuditBody {
    /// Decimal-as-string. The physical count.
    pub counted_quantity: String,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListAuditsQuery {
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
    body: web::Json<RecordAuditBody>,
    sse_state: web::Data<SseState>,
    notification_service: web::Data<NotificationService>,
) -> impl Responder {
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(c) => c.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };
    if !is_technician_or_admin(&claims) {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can record audits",
        );
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let asset_id = path.into_inner();
    let body = body.into_inner();

    // Parse the counted quantity. Audits accept zero (the row
    // is now empty) but not negatives.
    let counted = match BigDecimal::from_str(body.counted_quantity.trim()) {
        Ok(q) => q,
        Err(_) => return errors::bad_request("counted_quantity must be a decimal number"),
    };
    if counted < BigDecimal::from(0) {
        return errors::bad_request("counted_quantity cannot be negative");
    }

    // Gate: asset must be stock-tracked and not externally
    // owned. Both checks mirror the usage-record path so the
    // two ledgers stay aligned on which assets accept writes.
    use crate::schema::assets;
    use diesel::prelude::*;
    let row: Result<(Option<BigDecimal>, Option<String>), DieselError> = assets::table
        .find(asset_id)
        .select((assets::quantity, assets::external_sync_source))
        .first(&mut conn);
    let (current_qty, external) = match row {
        Ok(r) => r,
        Err(DieselError::NotFound) => {
            return errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        Err(e) => {
            error!(asset_id, error = ?e, "failed to load asset for audit");
            return errors::internal("Failed to load asset");
        }
    };
    if current_qty.is_none() {
        return errors::unprocessable_entity(
            "Asset is not stock-tracked: set a quantity on the asset before auditing",
        );
    }
    if external.is_some() {
        return errors::forbidden(
            "Asset is externally synced; corrections must be made in the source system",
        );
    }

    let recorded_by = uuid::Uuid::parse_str(&claims.sub).ok();
    let notes = body.notes.and_then(|s| {
        let trimmed = s.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });

    match repo::record_audit(&mut conn, asset_id, counted, notes, recorded_by) {
        Ok(outcome) => {
            sse_state
                .broadcast_event(SseEvent::AssetAuditRecorded {
                    audit_id: outcome.row.id,
                    asset_id,
                    asset_name: outcome.asset_name.clone(),
                    counted_quantity: outcome.row.counted_quantity.to_string(),
                    previous_quantity: outcome.row.previous_quantity.to_string(),
                    delta: outcome.row.delta.to_string(),
                    unit: outcome.asset_unit.clone(),
                    notes: outcome.row.notes.clone(),
                    recorded_at: outcome.row.recorded_at,
                    timestamp: chrono::Utc::now(),
                })
                .await;

            if outcome.crossed_low_stock {
                if let Some(threshold) = outcome.threshold.as_ref() {
                    let actor_uuid = recorded_by.unwrap_or_else(uuid::Uuid::nil);
                    let actor_name = actor_name_for(&mut conn, actor_uuid)
                        .unwrap_or_else(|| "System".to_string());
                    let recipients = inventory_alert_recipients(&mut conn);
                    let body_text = format!(
                        "{} audit lowered stock: {} {} remaining (threshold {} {}).",
                        outcome.asset_name,
                        outcome.new_quantity,
                        outcome.asset_unit,
                        threshold,
                        outcome.asset_unit,
                    );
                    for recipient_uuid in recipients {
                        let payload = NotificationPayload::new(
                            NotificationTypeCode::AssetLowStock,
                            recipient_uuid,
                            NotificationActor {
                                uuid: actor_uuid,
                                name: actor_name.clone(),
                                avatar_thumb: None,
                            },
                            NotificationEntity::Asset {
                                id: asset_id,
                                name: outcome.asset_name.clone(),
                            },
                        )
                        .with_body(body_text.clone());
                        if let Err(e) = notification_service.notify(payload).await {
                            error!(
                                asset_id,
                                recipient = %recipient_uuid,
                                error = %e,
                                "Failed to deliver asset_low_stock notification from audit",
                            );
                        }
                    }
                }
            }
            HttpResponse::Created().json(outcome.row)
        }
        Err(e) => {
            error!(asset_id, error = ?e, "failed to record asset audit");
            errors::internal("Failed to record audit")
        }
    }
}

pub async fn list_for_asset(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    query: web::Query<ListAuditsQuery>,
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
            error!(asset_id, error = ?e, "failed to list asset audits");
            errors::internal("Failed to load audit history")
        }
    }
}

// ---- recipient helpers (parallel to handlers::asset_usage) -

fn inventory_alert_recipients(conn: &mut crate::db::DbConnection) -> Vec<uuid::Uuid> {
    use crate::models::UserRole;
    use crate::schema::users;
    use diesel::prelude::*;
    let res: Result<Vec<uuid::Uuid>, diesel::result::Error> = users::table
        .filter(users::role.eq_any(vec![UserRole::Admin, UserRole::Technician]))
        .filter(users::deleted_at.is_null())
        .select(users::uuid)
        .load(conn);
    res.unwrap_or_else(|e| {
        error!(error = %e, "failed to load inventory alert recipients");
        Vec::new()
    })
}

fn actor_name_for(conn: &mut crate::db::DbConnection, actor: uuid::Uuid) -> Option<String> {
    use crate::schema::users;
    use diesel::prelude::*;
    if actor.is_nil() {
        return None;
    }
    users::table
        .find(actor)
        .select(users::name)
        .first::<String>(conn)
        .ok()
}
