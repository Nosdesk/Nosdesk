//! Asset usage endpoints.
//!
//! - `POST /api/assets/{id}/usage` — record a usage event.
//!   Technician + admin only. Refused for assets that aren't
//!   stock-tracked (assets.quantity IS NULL).
//! - `GET  /api/assets/{id}/usage` — paginated history for an
//!   asset.
//! - `GET  /api/tickets/{id}/asset-usage` — usage rows attached
//!   to a ticket.

use actix_web::{web, HttpResponse, Responder};
use bigdecimal::BigDecimal;
use diesel::result::Error as DieselError;
use serde::Deserialize;
use std::str::FromStr;
use tracing::error;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::NewAssetUsage;
use crate::repository::asset_usage as repo;
use crate::services::notifications::types::{
    NotificationActor, NotificationEntity, NotificationPayload, NotificationTypeCode,
};
use crate::services::notifications::NotificationService;

#[derive(Debug, Deserialize)]
pub struct RecordUsageBody {
    /// Decimal string. Diesel's BigDecimal can't safely round-
    /// trip through f64 for quantities measured to 3dp, so the
    /// wire format is text.
    pub quantity_used: String,
    /// Optional tie-in to a ticket. If absent, the row is an
    /// ad-hoc event (restock, write-off).
    #[serde(default)]
    pub ticket_id: Option<i32>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Direction discriminator. `"usage"` (default) decrements
    /// the asset's on-hand quantity; `"restock"` increments it.
    /// Unknown values are rejected at the handler boundary so
    /// the DB CHECK never has to be the only validator.
    #[serde(default = "default_event_kind")]
    pub kind: String,
}

fn default_event_kind() -> String {
    "usage".to_string()
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
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    body: web::Json<RecordUsageBody>,
    notification_service: web::Data<NotificationService>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can record usage",
        );
    }

    let asset_id = path.into_inner();
    let body = body.into_inner();

    // Validate the event kind at the handler boundary. The DB
    // CHECK also catches bad values, but a clean 422 here is
    // friendlier than a 500.
    if body.kind != "usage" && body.kind != "restock" {
        return errors::bad_request("kind must be 'usage' or 'restock'");
    }

    // Parse and bounds-check the quantity. Decimal-string in,
    // BigDecimal out; reject zero / negative immediately because
    // the CHECK constraint on the DB would also catch them but
    // a 422 with a clear message is friendlier than a 500.
    let quantity = match BigDecimal::from_str(body.quantity_used.trim()) {
        Ok(q) => q,
        Err(_) => return errors::bad_request("quantity_used must be a decimal number"),
    };
    if quantity <= 0 {
        return errors::bad_request("quantity_used must be greater than zero");
    }

    // Look up the asset's current quantity + unit. Refuse usage
    // writes for assets that aren't stock-tracked (quantity is
    // NULL); the unit lives on the asset's own row so we can
    // stamp it onto the usage row without trusting client input.
    let asset_row = tc.run(|conn| {
        use crate::schema::assets;
        use diesel::prelude::*;
        assets::table
            .find(asset_id)
            .select((assets::quantity, assets::unit))
            .first::<(Option<BigDecimal>, Option<String>)>(conn)
            .optional()
    });
    let (current_qty, unit) = match asset_row {
        Ok(Some(row)) => row,
        Ok(None) => return errors::not_found_msg(format!("Asset {asset_id} not found")),
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

    let recorded_by = Some(auth.user_uuid);
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
        event_kind: body.kind,
    };

    match tc.run(|conn| repo::record_event(conn, new_usage)) {
        Ok(outcome) => {
            // The ledger row reaches the usage-history panels through the
            // sync stream (record_event emits `asset_usage.recorded`),
            // cross-machine; no discrete SSE broadcast.

            if outcome.crossed_low_stock {
                if let Some(threshold) = outcome.threshold.as_ref() {
                    let threshold_str = threshold.to_string();
                    let quantity_str = outcome.new_quantity.to_string();
                    let asset_name = outcome.asset_name.clone();

                    // Low-stock alerts ride the notification system now
                    // (persisted + delivered cross-machine via the
                    // notification sync aggregate); the dedicated
                    // AssetLowStock SSE toast was retired.
                    //
                    // Fan persistent notifications out to every
                    // admin/technician who hasn't opted out. The
                    // actor is the user who recorded the usage
                    // event; the NotificationService self-skip
                    // suppresses their own copy. Failure to
                    // notify any recipient must not fail the
                    // usage write, so errors are logged not
                    // bubbled.
                    let actor_uuid = auth.user_uuid;
                    let actor_name = tc
                        .run(|conn| Ok(actor_name_for(conn, actor_uuid)))
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| "System".to_string());
                    let recipients = tc
                        .run(|conn| Ok(inventory_alert_recipients(conn)))
                        .unwrap_or_default();

                    let body = format!(
                        "{} is low: {} {} remaining (threshold {} {}).",
                        asset_name, quantity_str, unit, threshold_str, unit,
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
                                name: asset_name.clone(),
                            },
                        )
                        .with_body(body.clone());

                        if let Err(e) = notification_service.notify(payload).await {
                            error!(
                                asset_id,
                                recipient = %recipient_uuid,
                                error = %e,
                                "Failed to deliver asset_low_stock notification",
                            );
                        }
                    }
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
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<i32>,
    query: web::Query<ListUsageQuery>,
) -> impl Responder {
    let asset_id = path.into_inner();
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);

    match tc.run(|conn| repo::list_for_asset(conn, asset_id, limit, offset)) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(asset_id, error = ?e, "failed to list asset usage");
            errors::internal("Failed to load usage history")
        }
    }
}

pub async fn list_for_ticket(
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<i32>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    match tc.run(|conn| repo::list_for_ticket(conn, ticket_id)) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(ticket_id, error = ?e, "failed to list ticket usage");
            errors::internal("Failed to load usage history")
        }
    }
}

/// Look up the recipients for an inventory-level alert: every
/// active admin or technician. Inventory management isn't a
/// per-asset subscription model, those roles are who acts on
/// low-stock signals.
///
/// Errors swallow to an empty list rather than failing the
/// usage write that triggered the notification; we'd rather a
/// degraded notification path than a refused usage record.
fn inventory_alert_recipients(conn: &mut crate::db::DbConnection) -> Vec<uuid::Uuid> {
    use crate::schema::{users, workspace_members};
    use diesel::prelude::*;
    // Staff = platform admin OR workspace owner/admin/agent in the
    // request's workspace. Runs under TenantConn, which pins
    // `app.workspace_id`; reading that GUC scopes recipients to the
    // asset's workspace under hosted multi-tenancy (bootstrap workspace
    // in single-tenant).
    let res: Result<Vec<uuid::Uuid>, diesel::result::Error> = users::table
        .filter(users::deleted_at.is_null())
        .filter(
            users::platform_role
                .eq("platform_admin")
                .or(diesel::dsl::exists(
                    workspace_members::table
                        .filter(workspace_members::user_uuid.eq(users::uuid))
                        .filter(workspace_members::workspace_id.eq(diesel::dsl::sql::<
                            diesel::sql_types::Integer,
                        >(
                            "NULLIF(current_setting('app.workspace_id', true), '')::int",
                        )))
                        .filter(workspace_members::role.eq_any(vec!["owner", "admin", "agent"])),
                )),
        )
        .select(users::uuid)
        .load(conn);
    res.unwrap_or_else(|e| {
        error!(error = %e, "failed to load inventory alert recipients");
        Vec::new()
    })
}

/// Resolve the actor's display name for the notification
/// payload. Returns None when the actor is the nil UUID or the
/// lookup fails; the caller falls back to "System" for that
/// case so the notification still renders coherently.
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
