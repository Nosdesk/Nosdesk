//! Dashboard endpoints.
//!
//! Currently exposes `GET /api/dashboard/stats`, the consolidated
//! ticket-counts endpoint that backs the dashboard's stat widgets.
//! Frontend's widget registry derives an `include` set from the
//! user's active widgets and passes it here so we only compute
//! what's about to be displayed.

use std::collections::HashSet;

use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;
use tracing::error;
use uuid::Uuid;

use crate::extractors::{AuthContext, WorkspaceContext};
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::repository::dashboard_stats::{self, StatsGroup};

#[derive(Deserialize)]
pub struct StatsQuery {
    /// Comma-separated list of stat groups to compute. Example:
    /// `?include=queue,yours`. Omit to receive all groups.
    pub include: Option<String>,
    /// User-scoped groups (`yours`, `summary`) operate on this
    /// user. Defaults to the authed user when omitted. Non-admins
    /// may only read their own user-scoped stats.
    pub user: Option<Uuid>,
}

impl StatsQuery {
    fn parse_include(&self) -> actix_web::Result<HashSet<StatsGroup>> {
        let Some(raw) = self.include.as_deref() else {
            return Ok(StatsGroup::all());
        };
        let mut set = HashSet::new();
        for token in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match StatsGroup::parse(token) {
                Some(g) => {
                    set.insert(g);
                }
                None => {
                    let resp = HttpResponse::BadRequest().json(json!({
                        "error": "unknown include key",
                        "key": token,
                        "allowed": StatsGroup::all_keys(),
                    }));
                    return Err(errors::from_response("unknown include key", resp));
                }
            }
        }
        Ok(set)
    }
}

pub async fn get_stats(
    pool: web::Data<crate::db::Pool>,
    query: web::Query<StatsQuery>,
    auth: AuthContext,
    ws: WorkspaceContext,
) -> actix_web::Result<HttpResponse> {
    let target_user = query.user.unwrap_or(auth.user_uuid);

    if target_user != auth.user_uuid && !auth.can_handle_tickets() {
        return Ok(errors::forbidden("forbidden"));
    }

    let groups = query.parse_include()?;

    if groups.is_empty() {
        // Empty `include=` (e.g., `?include=`) is a request for
        // nothing; return an empty bundle rather than computing
        // everything by accident.
        return Ok(HttpResponse::Ok().json(dashboard_stats::StatsBundle::default()));
    }

    let mut conn = helpers::db_conn(&pool)?;
    // Pin the resolved workspace so the stats queries (tickets and related
    // RLS-isolated tables) are visible; the pool clears app.workspace_id on
    // checkout, so an unpinned conn computes empty stats in hosted mode.
    helpers::pin_workspace(&mut conn, ws.workspace_id);

    match dashboard_stats::compute(&mut conn, &target_user, &groups) {
        Ok(bundle) => Ok(HttpResponse::Ok().json(bundle)),
        Err(e) => {
            error!(error = ?e, "dashboard stats computation failed");
            Ok(errors::internal("stats unavailable"))
        }
    }
}
