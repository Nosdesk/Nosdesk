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

use crate::db::Pool;
use crate::extractors::AuthContext;
use crate::handlers::{errors, helpers};
use crate::repository::sla_admin::{
    self, SlaPolicyBody, WorkingCalendarBody,
};

fn require_admin(auth: &AuthContext) -> Option<HttpResponse> {
    if auth.is_admin() {
        None
    } else {
        Some(errors::forbidden("Admin role required"))
    }
}

// ---- Policies ----

pub async fn list_policies(pool: web::Data<Pool>, _auth: AuthContext) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match sla_admin::list_policies(&mut conn) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, "list sla policies failed");
            errors::internal("Failed to list SLA policies")
        }
    }
}

pub async fn create_policy(
    pool: web::Data<Pool>,
    body: web::Json<SlaPolicyBody>,
    auth: AuthContext,
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match sla_admin::create_policy(&mut conn, body.into_inner(), Some(auth.user_uuid)) {
        Ok(policy) => HttpResponse::Created().json(policy),
        Err(e) => {
            error!(error = %e, "create sla policy failed");
            errors::internal("Failed to create SLA policy")
        }
    }
}

pub async fn update_policy(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<SlaPolicyBody>,
    auth: AuthContext,
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    let id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match sla_admin::update_policy(&mut conn, id, body.into_inner()) {
        Ok(policy) => HttpResponse::Ok().json(policy),
        Err(e) => {
            error!(error = %e, id, "update sla policy failed");
            errors::internal("Failed to update SLA policy")
        }
    }
}

pub async fn delete_policy(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    let id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match sla_admin::delete_policy(&mut conn, id) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = %e, id, "delete sla policy failed");
            errors::internal("Failed to delete SLA policy")
        }
    }
}

// ---- Calendars ----

pub async fn list_calendars(pool: web::Data<Pool>, _auth: AuthContext) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match sla_admin::list_calendars(&mut conn) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, "list calendars failed");
            errors::internal("Failed to list working calendars")
        }
    }
}

pub async fn create_calendar(
    pool: web::Data<Pool>,
    body: web::Json<WorkingCalendarBody>,
    auth: AuthContext,
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match sla_admin::create_calendar(&mut conn, body.into_inner(), Some(auth.user_uuid)) {
        Ok(cal) => HttpResponse::Created().json(cal),
        Err(e) => {
            error!(error = %e, "create calendar failed");
            errors::internal("Failed to create working calendar")
        }
    }
}

pub async fn update_calendar(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<WorkingCalendarBody>,
    auth: AuthContext,
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    let id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match sla_admin::update_calendar(&mut conn, id, body.into_inner()) {
        Ok(cal) => HttpResponse::Ok().json(cal),
        Err(e) => {
            error!(error = %e, id, "update calendar failed");
            errors::internal("Failed to update working calendar")
        }
    }
}

pub async fn delete_calendar(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    let id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match sla_admin::delete_calendar(&mut conn, id) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = %e, id, "delete calendar failed");
            errors::internal("Failed to delete working calendar")
        }
    }
}
