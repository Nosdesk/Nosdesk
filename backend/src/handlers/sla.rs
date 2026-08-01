//! Admin endpoints for SLA policies + working calendars.
//!
//! - `GET    /api/admin/sla/policies` — list every policy.
//! - `POST   /api/admin/sla/policies` — create a policy.
//! - `PATCH  /api/admin/sla/policies/{id}` — replace fields on
//!   the policy. Body is the full shape; missing fields clear.
//! - `DELETE /api/admin/sla/policies/{id}` — drop the row.
//! - `GET    /api/admin/sla/policies/matches` — per-policy match
//!   counts (total, on-track, at-risk, breached, paused) for the
//!   open tickets in the workspace. Drives the live state pills on
//!   the policy list.
//! - `GET    /api/sla/workspace-summary` — non-admin (technician+
//!   gated): the same scan rolled up to a single workspace total.
//!   Drives the dashboard SLA health widget.
//! - `GET    /api/admin/sla/calendars` — list calendars.
//! - `POST   /api/admin/sla/calendars` — create.
//! - `PATCH  /api/admin/sla/calendars/{id}` — update.
//! - `DELETE /api/admin/sla/calendars/{id}` — drop.
//! - `GET    /api/admin/sla/calendars/{id}/holidays` — list dates.
//! - `POST   /api/admin/sla/calendars/{id}/holidays` — add one.
//! - `DELETE /api/admin/sla/holidays/{id}` — remove one.
//! - `GET    /api/tickets/{id}/sla/explain` — non-admin: why a
//!   ticket currently has the SLA pill it does. Surfaces the matched
//!   policy + calendar + per-state pause flag so the user can audit
//!   the engine's decision without leaving the ticket view.
//!
//! All writes gate on admin via the AuthContext.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use diesel::prelude::*;
use serde::Serialize;
use tracing::error;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::{SlaPolicy, Ticket, WorkflowState, WorkspaceRole};
use crate::repository::sla_admin::{
    self, SlaPolicyBody, WorkingCalendarBody, WorkingCalendarHolidayBody,
};
use crate::utils::rbac::require_workspace_role;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/sla/policies",
        web::get().to(crate::handlers::sla::list_policies),
    )
    .route(
        "/admin/sla/policies",
        web::post().to(crate::handlers::sla::create_policy),
    )
    // Static path must precede /{id} so "matches" isn't parsed as an id.
    .route(
        "/admin/sla/policies/matches",
        web::get().to(crate::handlers::sla::policy_match_counts),
    )
    .route(
        "/admin/sla/policies/{id}",
        web::patch().to(crate::handlers::sla::update_policy),
    )
    .route(
        "/admin/sla/policies/{id}",
        web::delete().to(crate::handlers::sla::delete_policy),
    )
    .route(
        "/admin/sla/calendars",
        web::get().to(crate::handlers::sla::list_calendars),
    )
    .route(
        "/admin/sla/calendars",
        web::post().to(crate::handlers::sla::create_calendar),
    )
    .route(
        "/admin/sla/calendars/{id}",
        web::patch().to(crate::handlers::sla::update_calendar),
    )
    .route(
        "/admin/sla/calendars/{id}",
        web::delete().to(crate::handlers::sla::delete_calendar),
    )
    .route(
        "/admin/sla/calendars/{id}/holidays",
        web::get().to(crate::handlers::sla::list_holidays),
    )
    .route(
        "/admin/sla/calendars/{id}/holidays",
        web::post().to(crate::handlers::sla::create_holiday),
    )
    .route(
        "/admin/sla/holidays/{id}",
        web::delete().to(crate::handlers::sla::delete_holiday),
    )
    .route(
        "/tickets/{id}/sla/explain",
        web::get().to(crate::handlers::sla::explain_for_ticket),
    )
    .route(
        "/sla/workspace-summary",
        web::get().to(crate::handlers::sla::workspace_summary),
    );
}

// ---- Policies ----

pub async fn list_policies(mut tc: TenantConn, _auth: AuthContext) -> impl Responder {
    match tc.run(sla_admin::list_policies) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, "list sla policies failed");
            errors::internal("Failed to list SLA policies")
        }
    }
}

pub async fn create_policy(
    mut tc: TenantConn,
    body: web::Json<SlaPolicyBody>,
    auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    let actor_uuid = auth.user_uuid;
    match tc.run(|conn| sla_admin::create_policy(conn, body.into_inner(), Some(actor_uuid))) {
        Ok(policy) => HttpResponse::Created().json(policy),
        Err(e) => {
            error!(error = %e, "create sla policy failed");
            errors::internal("Failed to create SLA policy")
        }
    }
}

pub async fn update_policy(
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<SlaPolicyBody>,
    _auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    let id = path.into_inner();
    match tc.run(|conn| sla_admin::update_policy(conn, id, body.into_inner())) {
        Ok(policy) => HttpResponse::Ok().json(policy),
        Err(e) => {
            error!(error = %e, id, "update sla policy failed");
            errors::internal("Failed to update SLA policy")
        }
    }
}

pub async fn delete_policy(
    mut tc: TenantConn,
    path: web::Path<i32>,
    _auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    let id = path.into_inner();
    match tc.run(|conn| sla_admin::delete_policy(conn, id)) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = %e, id, "delete sla policy failed");
            errors::internal("Failed to delete SLA policy")
        }
    }
}

// ---- Calendars ----

pub async fn list_calendars(mut tc: TenantConn, _auth: AuthContext) -> impl Responder {
    match tc.run(sla_admin::list_calendars) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, "list calendars failed");
            errors::internal("Failed to list working calendars")
        }
    }
}

pub async fn create_calendar(
    mut tc: TenantConn,
    body: web::Json<WorkingCalendarBody>,
    auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    let actor_uuid = auth.user_uuid;
    match tc.run(|conn| sla_admin::create_calendar(conn, body.into_inner(), Some(actor_uuid))) {
        Ok(cal) => HttpResponse::Created().json(cal),
        Err(e) => {
            error!(error = %e, "create calendar failed");
            errors::internal("Failed to create working calendar")
        }
    }
}

pub async fn update_calendar(
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<WorkingCalendarBody>,
    _auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    let id = path.into_inner();
    match tc.run(|conn| sla_admin::update_calendar(conn, id, body.into_inner())) {
        Ok(cal) => HttpResponse::Ok().json(cal),
        Err(e) => {
            error!(error = %e, id, "update calendar failed");
            errors::internal("Failed to update working calendar")
        }
    }
}

pub async fn delete_calendar(
    mut tc: TenantConn,
    path: web::Path<i32>,
    _auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    let id = path.into_inner();
    match tc.run(|conn| sla_admin::delete_calendar(conn, id)) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = %e, id, "delete calendar failed");
            errors::internal("Failed to delete working calendar")
        }
    }
}

// ---- Holidays ----

pub async fn list_holidays(
    mut tc: TenantConn,
    path: web::Path<i32>,
    _auth: AuthContext,
) -> impl Responder {
    let calendar_id = path.into_inner();
    match tc.run(|conn| sla_admin::list_holidays(conn, calendar_id)) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, calendar_id, "list holidays failed");
            errors::internal("Failed to list holidays")
        }
    }
}

pub async fn create_holiday(
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<WorkingCalendarHolidayBody>,
    _auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    let calendar_id = path.into_inner();
    match tc.run(|conn| sla_admin::create_holiday(conn, calendar_id, body.into_inner())) {
        Ok(row) => HttpResponse::Created().json(row),
        Err(diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        )) => errors::bad_request("Holiday already exists for that date"),
        Err(e) => {
            error!(error = %e, calendar_id, "create holiday failed");
            errors::internal("Failed to create holiday")
        }
    }
}

pub async fn delete_holiday(
    mut tc: TenantConn,
    path: web::Path<i32>,
    _auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    let id = path.into_inner();
    match tc.run(|conn| sla_admin::delete_holiday(conn, id)) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = %e, id, "delete holiday failed");
            errors::internal("Failed to delete holiday")
        }
    }
}

// ---- Live match counts ----

/// Upper bound on the ticket set the per-policy scan walks per call.
/// At 10k tickets the in-memory loop is still milliseconds, and the
/// counts stay useful as approximations above that. A workspace that
/// regularly hits this should move to materialised counts.
const POLICY_MATCH_SCAN_LIMIT: i64 = 10_000;

/// GET /api/admin/sla/policies/matches. Returns a map from policy
/// id to counts. Delegates to `services::sla::scan_open_ticket_buckets`
/// so the per-policy admin view and the workspace dashboard widget
/// share the same scan and bucketing rules.
pub async fn policy_match_counts(mut tc: TenantConn, _auth: AuthContext) -> impl Responder {
    let result = tc
        .run(|conn| crate::services::sla::scan_open_ticket_buckets(conn, POLICY_MATCH_SCAN_LIMIT));
    match result {
        Ok(scan) => HttpResponse::Ok().json(scan.by_policy),
        Err(e) => {
            error!(error = %e, "policy match counts failed");
            errors::internal("Failed to compute policy match counts")
        }
    }
}

/// GET /api/sla/workspace-summary. Workspace-wide roll-up of the
/// same per-policy scan: total open tickets covered by any policy,
/// partitioned by pill state. Gated to technicians + admins so the
/// dashboard widget can show "3 breached, 7 at risk" at a glance
/// without exposing workspace-level urgency to end-users.
pub async fn workspace_summary(
    mut tc: TenantConn,
    _auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return resp;
    }
    let result = tc
        .run(|conn| crate::services::sla::scan_open_ticket_buckets(conn, POLICY_MATCH_SCAN_LIMIT));
    match result {
        Ok(scan) => HttpResponse::Ok().json(scan.workspace_total),
        Err(e) => {
            error!(error = %e, "sla workspace summary failed");
            errors::internal("Failed to compute SLA workspace summary")
        }
    }
}

// ---- Explain ("why this SLA?") ----

#[derive(Debug, Serialize)]
pub struct SlaExplain {
    /// Matched policy, or `None` when nothing matched and there's no
    /// workspace default (no pill renders client-side either).
    pub policy: Option<SlaExplainPolicy>,
    pub state: SlaExplainState,
}

#[derive(Debug, Serialize)]
pub struct SlaExplainPolicy {
    pub id: i32,
    pub name: String,
    pub is_default: bool,
    /// When true the matched policy grants NO SLA — the popover explains why the
    /// ticket has no timer despite a policy matching.
    pub no_sla: bool,
    pub target_response_minutes: Option<i32>,
    pub target_resolution_minutes: Option<i32>,
    pub calendar: Option<SlaExplainCalendar>,
    /// Filters the matcher saw as a hit. Empty when the policy is the
    /// workspace default with no filters set.
    pub matched_filters: Vec<SlaExplainFilter>,
}

#[derive(Debug, Serialize)]
pub struct SlaExplainCalendar {
    pub id: i32,
    pub name: String,
    pub timezone: String,
}

#[derive(Debug, Serialize)]
pub struct SlaExplainState {
    pub paused: bool,
    pub state_name: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlaExplainFilter {
    Priority { value: String },
    Category { id: i32, name: String },
    AssigneeGroup { id: i32, name: String },
}

/// GET /api/tickets/{id}/sla/explain. Authenticated; the matcher
/// runs inside the user's tenant connection so RLS keeps cross-
/// workspace visibility off.
pub async fn explain_for_ticket(
    mut tc: TenantConn,
    path: web::Path<i32>,
    _auth: AuthContext,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let result: Result<Option<SlaExplain>, diesel::result::Error> = tc.run(|conn| {
        use crate::schema::{groups, sla_policies, ticket_categories, tickets, workflow_states};

        let ticket: Ticket = match tickets::table.find(ticket_id).first::<Ticket>(conn) {
            Ok(t) => t,
            Err(diesel::result::Error::NotFound) => return Ok(None),
            Err(e) => return Err(e),
        };

        let state: WorkflowState = workflow_states::table
            .find(ticket.workflow_state_id)
            .first(conn)?;

        let policies: Vec<SlaPolicy> = sla_policies::table.load(conn)?;
        let group_ids = ticket
            .assignee_uuid
            .and_then(|u| crate::repository::groups::get_group_ids_for_user(conn, &u).ok())
            .unwrap_or_default();

        let explain_policy =
            crate::services::sla::pick_policy(&policies, &ticket, &group_ids).map(|policy| {
                let calendar = policy.working_calendar_id.and_then(|cid| {
                    crate::schema::working_calendars::table
                        .find(cid)
                        .first::<crate::models::WorkingCalendar>(conn)
                        .ok()
                        .map(|c| SlaExplainCalendar {
                            id: c.id,
                            name: c.name,
                            timezone: c.timezone,
                        })
                });

                // Translate the filters the matcher accepted into typed
                // entries so the frontend can render them with local
                // copy + links rather than parsing strings.
                let mut matched_filters: Vec<SlaExplainFilter> = Vec::new();
                if let Some(ref p) = policy.priority_filter {
                    matched_filters.push(SlaExplainFilter::Priority { value: p.clone() });
                }
                if let Some(cid) = policy.category_id_filter {
                    let name = ticket_categories::table
                        .find(cid)
                        .select(ticket_categories::name)
                        .first::<String>(conn)
                        .unwrap_or_else(|_| format!("#{cid}"));
                    matched_filters.push(SlaExplainFilter::Category { id: cid, name });
                }
                if let Some(gid) = policy.assignee_group_id_filter {
                    let name = groups::table
                        .find(gid)
                        .select(groups::name)
                        .first::<String>(conn)
                        .unwrap_or_else(|_| format!("#{gid}"));
                    matched_filters.push(SlaExplainFilter::AssigneeGroup { id: gid, name });
                }

                SlaExplainPolicy {
                    id: policy.id,
                    name: policy.name.clone(),
                    is_default: policy.is_default,
                    no_sla: policy.no_sla,
                    target_response_minutes: policy.target_response_minutes,
                    target_resolution_minutes: policy.target_resolution_minutes,
                    calendar,
                    matched_filters,
                }
            });

        Ok(Some(SlaExplain {
            policy: explain_policy,
            state: SlaExplainState {
                paused: state.pauses_sla,
                state_name: state.name,
            },
        }))
    });

    match result {
        Ok(Some(explain)) => HttpResponse::Ok().json(explain),
        Ok(None) => errors::not_found_msg("Ticket not found"),
        Err(e) => {
            error!(error = %e, ticket_id, "sla explain failed");
            errors::internal("Failed to load SLA explain")
        }
    }
}

#[cfg(test)]
mod tests {
    //! Permission-boundary tests. Unlike most admin handlers, SLA
    //! endpoints gate via the `AuthContext` extractor (which queries
    //! the DB for the user's role rather than reading it from claims
    //! directly). The shared `claims_for` helper still works because
    //! it creates a real user row that AuthContext then loads.
    use super::*;
    use crate::test_helpers::{claims_for, setup_test_pool};
    use actix_web::test as actix_test;
    use actix_web::{http::StatusCode, App, HttpMessage};

    fn test_app(
        pool: crate::db::Pool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/admin/sla/policies/{id}", web::delete().to(delete_policy))
    }

    #[actix_web::test]
    async fn delete_policy_requires_authentication() {
        // No claims in extensions -> AuthContext extractor errors with 401
        // (its Unauthorized variant), gate never reached.
        let pool = setup_test_pool();
        let app = actix_test::init_service(test_app(pool)).await;
        let req = actix_test::TestRequest::delete()
            .uri("/admin/sla/policies/1")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn delete_policy_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, "user");
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::delete()
            .uri("/admin/sla/policies/1")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn delete_policy_rejects_technician_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, "technician");
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::delete()
            .uri("/admin/sla/policies/1")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
