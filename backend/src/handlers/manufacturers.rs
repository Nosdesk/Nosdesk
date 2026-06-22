//! Manufacturers CRUD (asset model catalog).
//!
//! A manufacturer is a make (Apple, Dell). Technician-gated like asset
//! creation: anyone who can create an asset can build the catalog the
//! create picker draws from. Reads + writes share that gate.
//!
//! - `GET    /api/manufacturers`        list
//! - `GET    /api/manufacturers/{id}`   fetch one
//! - `POST   /api/manufacturers`        create
//! - `PUT    /api/manufacturers/{id}`   rename
//! - `DELETE /api/manufacturers/{id}`   delete (refused while models reference it)

use actix_web::{web, HttpResponse, Responder};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde::Deserialize;
use tracing::error;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::{ManufacturerChange, NewManufacturer};
use crate::repository::manufacturers as repo;

const NAME_MAX_LEN: usize = 255;

#[derive(Debug, Deserialize)]
pub struct UpsertBody {
    pub name: String,
}

fn validate_name(name: &str) -> Result<String, HttpResponse> {
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() || trimmed.len() > NAME_MAX_LEN {
        return Err(errors::bad_request(format!(
            "name must be 1 to {NAME_MAX_LEN} characters"
        )));
    }
    Ok(trimmed)
}

pub async fn list(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    match tc.run(repo::list) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, "failed to list manufacturers");
            errors::internal("Failed to list manufacturers")
        }
    }
}

pub async fn get(mut tc: TenantConn, path: web::Path<i32>, auth: AuthContext) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    let id = path.into_inner();
    match tc.run(|conn| repo::get(conn, id)) {
        Ok(row) => HttpResponse::Ok().json(row),
        Err(DieselError::NotFound) => errors::not_found_msg(format!("Manufacturer {id} not found")),
        Err(e) => {
            error!(id, error = %e, "failed to load manufacturer");
            errors::internal("Failed to load manufacturer")
        }
    }
}

pub async fn create(
    mut tc: TenantConn,
    body: web::Json<UpsertBody>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    let name = match validate_name(&body.name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    let new = NewManufacturer {
        name,
        created_by: Some(auth.user_uuid),
    };
    match tc.run(|conn| repo::create(conn, new)) {
        Ok(row) => HttpResponse::Created().json(row),
        Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
            errors::bad_request("A manufacturer with that name already exists")
        }
        Err(e) => {
            error!(error = %e, "failed to create manufacturer");
            errors::internal("Failed to create manufacturer")
        }
    }
}

pub async fn update(
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<UpsertBody>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    let id = path.into_inner();
    let name = match validate_name(&body.name) {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    let change = ManufacturerChange {
        name: Some(name),
        updated_at: None,
    };
    match tc.run(|conn| repo::update(conn, id, change)) {
        Ok(row) => HttpResponse::Ok().json(row),
        Err(DieselError::NotFound) => errors::not_found_msg(format!("Manufacturer {id} not found")),
        Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
            errors::bad_request("A manufacturer with that name already exists")
        }
        Err(e) => {
            error!(id, error = %e, "failed to update manufacturer");
            errors::internal("Failed to update manufacturer")
        }
    }
}

pub async fn delete(mut tc: TenantConn, path: web::Path<i32>, auth: AuthContext) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    let id = path.into_inner();

    // The FK is RESTRICT, so a manufacturer with models can't be deleted.
    // Surface a clear message rather than letting the DB error bubble.
    match tc.run(|conn| repo::count_models(conn, id)) {
        Ok(0) => {}
        Ok(n) => {
            return errors::bad_request(format!(
                "This manufacturer has {n} model(s). Delete or reassign them first."
            ))
        }
        Err(e) => {
            error!(id, error = %e, "failed to count manufacturer models");
            return errors::internal("Failed to delete manufacturer");
        }
    }

    match tc.run(|conn| repo::delete(conn, id)) {
        Ok(0) => errors::not_found_msg(format!("Manufacturer {id} not found")),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(id, error = %e, "failed to delete manufacturer");
            errors::internal("Failed to delete manufacturer")
        }
    }
}
