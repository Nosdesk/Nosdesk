//! Canned-response CRUD. Reads open to any authenticated user so the
//! ticket composer's picker works for all techs; writes restricted
//! to admins so the template library has a single source of truth.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use serde::Deserialize;
use tracing::{error, info};

use crate::db::Pool;
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::models::{CannedResponse, CannedResponseUpdate, NewCannedResponse};
use crate::repository::canned_responses as repo;

/// Body for `POST /api/canned-responses`. Validation is trivial —
/// title + body both required and non-empty — and happens inline.
#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateRequest {
    pub title: Option<String>,
    pub body: Option<String>,
}

// ---------- Small response helpers (mirrors handlers/channels.rs) ----------

fn collapse(r: Result<HttpResponse, HttpResponse>) -> HttpResponse {
    match r {
        Ok(r) | Err(r) => r,
    }
}

fn server_error(msg: &str) -> HttpResponse {
    errors::internal(msg)
}

fn bad_request(msg: impl Into<String>) -> HttpResponse {
    errors::bad_request(msg)
}

// ---------- Routes ----------

/// GET /api/canned-responses — available to any authenticated user so
/// the reply composer can show the picker.
pub async fn list_canned(pool: web::Data<Pool>, req: HttpRequest) -> HttpResponse {
    collapse(list_impl(pool, req).await)
}

async fn list_impl(pool: web::Data<Pool>, req: HttpRequest) -> Result<HttpResponse, HttpResponse> {
    // auth_conn requires any authenticated user — read is open to
    // all techs / admins. We only care that the caller is logged in.
    let (_claims, _uuid, mut conn) = helpers::auth_conn(&req, &pool)?;
    let rows = repo::list(&mut conn).map_err(|e| {
        error!(error = %e, "failed to list canned_responses");
        server_error("Failed to list canned responses")
    })?;
    Ok(HttpResponse::Ok().json(rows))
}

/// POST /api/admin/canned-responses — admin only.
pub async fn create_canned(
    pool: web::Data<Pool>,
    body: web::Json<CreateRequest>,
    req: HttpRequest,
) -> HttpResponse {
    collapse(create_impl(pool, body, req).await)
}

async fn create_impl(
    pool: web::Data<Pool>,
    body: web::Json<CreateRequest>,
    req: HttpRequest,
) -> Result<HttpResponse, HttpResponse> {
    let mut conn = helpers::admin_conn(&req, &pool)?;
    let creator = req
        .extensions()
        .get::<crate::models::Claims>()
        .and_then(|c| uuid::Uuid::parse_str(&c.sub).ok());

    let title = body.title.trim();
    let content = body.body.trim();
    if title.is_empty() {
        return Err(bad_request("Title is required"));
    }
    if content.is_empty() {
        return Err(bad_request("Body is required"));
    }

    let created = repo::create(
        &mut conn,
        NewCannedResponse {
            title: title.to_string(),
            body: content.to_string(),
            created_by: creator,
        },
    )
    .map_err(|e| {
        error!(error = %e, "failed to create canned_response");
        server_error("Failed to create canned response")
    })?;

    info!(id = created.id, "canned response created");
    Ok(HttpResponse::Created().json(created))
}

/// PATCH /api/admin/canned-responses/{id} — admin only.
pub async fn update_canned(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<UpdateRequest>,
    req: HttpRequest,
) -> HttpResponse {
    collapse(update_impl(pool, path, body, req).await)
}

async fn update_impl(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<UpdateRequest>,
    req: HttpRequest,
) -> Result<HttpResponse, HttpResponse> {
    let mut conn = helpers::admin_conn(&req, &pool)?;
    let id = path.into_inner();

    if let Some(t) = body.title.as_deref() {
        if t.trim().is_empty() {
            return Err(bad_request("Title must not be empty"));
        }
    }
    if let Some(b) = body.body.as_deref() {
        if b.trim().is_empty() {
            return Err(bad_request("Body must not be empty"));
        }
    }

    let change = CannedResponseUpdate {
        title: body.title.clone().map(|s| s.trim().to_string()),
        body: body.body.clone().map(|s| s.trim().to_string()),
        ..Default::default()
    };

    let updated: CannedResponse = repo::update(&mut conn, id, change).map_err(|e| {
        error!(error = %e, "failed to update canned_response");
        match e {
            diesel::result::Error::NotFound => HttpResponse::NotFound().finish(),
            _ => server_error("Failed to update canned response"),
        }
    })?;
    Ok(HttpResponse::Ok().json(updated))
}

/// DELETE /api/admin/canned-responses/{id} — admin only.
pub async fn delete_canned(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    req: HttpRequest,
) -> HttpResponse {
    collapse(delete_impl(pool, path, req).await)
}

async fn delete_impl(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    req: HttpRequest,
) -> Result<HttpResponse, HttpResponse> {
    let mut conn = helpers::admin_conn(&req, &pool)?;
    let id = path.into_inner();
    let removed = repo::delete(&mut conn, id).map_err(|e| {
        error!(error = %e, "failed to delete canned_response");
        server_error("Failed to delete canned response")
    })?;
    if removed == 0 {
        return Ok(HttpResponse::NotFound().finish());
    }
    info!(id, "canned response deleted");
    Ok(HttpResponse::NoContent().finish())
}
