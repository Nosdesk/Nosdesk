//! Asset audit endpoints.
//!
//! - `POST /api/assets/{id}/audit` — record a physical-count
//!   audit. Technician + admin only. Refused for assets that
//!   aren't stock-tracked (assets.quantity IS NULL) and for
//!   externally-synced rows where manual correction would just
//!   be overwritten by the next sync.
//! - `GET  /api/assets/{id}/audits` — paginated history.

use actix_web::{web, HttpResponse, Responder};
use bigdecimal::BigDecimal;
use serde::Deserialize;
use std::str::FromStr;
use tracing::error;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::repository::asset_audits as repo;
use crate::services::notifications::types::{
    NotificationActor, NotificationEntity, NotificationPayload, NotificationTypeCode,
};
use crate::services::notifications::NotificationService;

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
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    body: web::Json<RecordAuditBody>,
    notification_service: web::Data<NotificationService>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can record audits",
        );
    }

    let asset_id = path.into_inner();
    let body = body.into_inner();

    // Parse the counted quantity. Audits accept zero (the row
    // is now empty) but not negatives.
    let counted = match BigDecimal::from_str(body.counted_quantity.trim()) {
        Ok(q) => q,
        Err(_) => return errors::bad_request("counted_quantity must be a decimal number"),
    };
    if counted < 0 {
        return errors::bad_request("counted_quantity cannot be negative");
    }

    // Gate: asset must be stock-tracked and not externally
    // owned. Both checks mirror the usage-record path so the
    // two ledgers stay aligned on which assets accept writes.
    let row = tc.run(|conn| {
        use crate::schema::assets;
        use diesel::prelude::*;
        assets::table
            .find(asset_id)
            .select((assets::quantity, assets::external_sync_source))
            .first::<(Option<BigDecimal>, Option<String>)>(conn)
            .optional()
    });
    let (current_qty, external) = match row {
        Ok(Some(r)) => r,
        Ok(None) => return errors::not_found_msg(format!("Asset {asset_id} not found")),
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

    let recorded_by = Some(auth.user_uuid);
    let notes = body.notes.and_then(|s| {
        let trimmed = s.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });

    match tc.run(|conn| repo::record_audit(conn, asset_id, counted, notes, recorded_by)) {
        Ok(outcome) => {
            // The audit row reaches the usage-history panels through the
            // sync stream (record_audit emits `asset_audit.recorded`),
            // cross-machine; no discrete SSE broadcast.

            if outcome.crossed_low_stock {
                if let Some(threshold) = outcome.threshold.as_ref() {
                    let actor_uuid = auth.user_uuid;
                    let actor_name = tc
                        .run(|conn| Ok(actor_name_for(conn, actor_uuid)))
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| "System".to_string());
                    let recipients = tc
                        .run(|conn| Ok(inventory_alert_recipients(conn)))
                        .unwrap_or_default();
                    let body_text = format!(
                        "{} audit lowered stock: {} {} remaining (threshold {} {}).",
                        outcome.asset_name,
                        outcome.new_quantity,
                        outcome.asset_unit,
                        threshold,
                        outcome.asset_unit,
                    );
                    let asset_workspace = tc
                        .workspace_id()
                        .unwrap_or(crate::sync::actor::BOOTSTRAP_WORKSPACE_ID);
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
                            asset_workspace,
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
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<i32>,
    query: web::Query<ListAuditsQuery>,
) -> impl Responder {
    let asset_id = path.into_inner();
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);
    match tc.run(|conn| repo::list_for_asset(conn, asset_id, limit, offset)) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(asset_id, error = ?e, "failed to list asset audits");
            errors::internal("Failed to load audit history")
        }
    }
}

// ---- recipient helpers (parallel to handlers::asset_usage) -

fn inventory_alert_recipients(conn: &mut crate::db::DbConnection) -> Vec<uuid::Uuid> {
    use crate::schema::{users, workspace_members};
    use diesel::prelude::*;
    // Staff = platform admin OR workspace owner/admin/agent in the
    // request's workspace. The handler runs under TenantConn, which
    // pins `app.workspace_id`, so reading that GUC scopes the recipient
    // set to the asset's workspace under hosted multi-tenancy (and to
    // the bootstrap workspace in single-tenant). Post-W2 replacement
    // for the legacy `users.role IN (admin, technician)` filter.
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
