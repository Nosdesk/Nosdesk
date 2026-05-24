//! Admin endpoints for SLA policies + working calendars.
//!
//! - `GET    /api/admin/sla/policies` — list every policy.
//! - `POST   /api/admin/sla/policies` — create a policy.
//! - `PATCH  /api/admin/sla/policies/{id}` — replace fields on
//!   the policy. Body is the full shape; missing fields clear.
//! - `DELETE /api/admin/sla/policies/{id}` — drop the row.
//! - `GET    /api/admin/sla/calendars` — list calendars.
//! - `POST   /api/admin/sla/calendars` — create.
//! - `PATCH  /api/admin/sla/calendars/{id}` — update.
//! - `DELETE /api/admin/sla/calendars/{id}` — drop.
//!
//! All writes gate on admin via the AuthContext.

use actix_web::{web, HttpResponse, Responder};
use tracing::error;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::repository::sla_admin::{self, SlaPolicyBody, WorkingCalendarBody};

fn require_admin(auth: &AuthContext) -> Option<HttpResponse> {
    if auth.is_admin() {
        None
    } else {
        Some(errors::forbidden("Admin role required"))
    }
}

// ---- Policies ----

pub async fn list_policies(mut tc: TenantConn, _auth: AuthContext) -> impl Responder {
    match tc.run(|conn| sla_admin::list_policies(conn)) {
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
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
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
    auth: AuthContext,
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
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
    auth: AuthContext,
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
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
    match tc.run(|conn| sla_admin::list_calendars(conn)) {
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
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
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
    auth: AuthContext,
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
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
    auth: AuthContext,
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
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

#[cfg(test)]
mod tests {
    //! Permission-boundary tests. Unlike most admin handlers, SLA
    //! endpoints gate via the `AuthContext` extractor (which queries
    //! the DB for the user's role rather than reading it from claims
    //! directly). The shared `claims_for` helper still works because
    //! it creates a real user row that AuthContext then loads.
    use super::*;
    use crate::models::UserRole;
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
        let claims = claims_for(&pool, UserRole::User);
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
        let claims = claims_for(&pool, UserRole::Technician);
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::delete()
            .uri("/admin/sla/policies/1")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
