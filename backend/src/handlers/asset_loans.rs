//! Device loan endpoints: issue a loaner, return it, edit an active loan,
//! and read an asset's loan history.
//!
//! A loan is the source of truth for an asset's custody; issuing and
//! returning also move `assets.status` and log the transition (see
//! `repository::asset_loans`). Writes are gated to agents/admins; reads
//! are open to any authenticated member.

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use tracing::error;
use uuid::Uuid;

use crate::extractors::{AuthContext, TenantConn, TicketAccess};
use crate::handlers::errors;
use crate::repository::{
    asset_loans::{self as repo, IssueLoan, LoanError},
    assets as assets_repo,
};

#[derive(Debug, Deserialize)]
pub struct IssueBody {
    pub borrower_user_uuid: Uuid,
    /// Start date (YYYY-MM-DD). Omit to start the loan now. A past date
    /// backdates an already-handed-over loan.
    pub loaned_at: Option<NaiveDate>,
    pub due_back: Option<NaiveDate>,
    pub ticket_id: Option<i32>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReturnBody {
    /// When the device came back; defaults to now.
    pub returned_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EditBody {
    /// New due date. Absent leaves it unchanged; clearing a due date is not
    /// supported here (issue a fresh loan or return + re-loan instead).
    pub due_back: Option<NaiveDate>,
    pub notes: Option<String>,
}

fn map_loan_err(asset_id: i32, e: LoanError) -> HttpResponse {
    match e {
        LoanError::AssetNotFound => errors::not_found_msg(format!("Asset {asset_id} not found")),
        LoanError::LoanNotFound => errors::not_found_msg("Loan not found"),
        LoanError::NotLoanable(status) => {
            errors::conflict(format!("Asset can't be loaned out from status '{status}'"))
        }
        LoanError::AlreadyOnLoan => errors::conflict("Asset is already out on loan"),
        LoanError::AlreadyReturned => errors::conflict("Loan has already been returned"),
        LoanError::InvalidReference => errors::bad_request("Unknown borrower or ticket"),
        LoanError::Db(err) => {
            error!(asset_id, error = ?err, "asset loan operation failed");
            errors::internal("Loan operation failed")
        }
    }
}

pub async fn list_for_asset(
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<i32>,
) -> impl Responder {
    let asset_id = path.into_inner();
    match tc.run(|conn| {
        assets_repo::get_device_by_id(conn, asset_id)?;
        repo::list_for_asset(conn, asset_id)
    }) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(diesel::result::Error::NotFound) => {
            errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        Err(e) => {
            error!(asset_id, error = ?e, "failed to list asset loans");
            errors::internal("Failed to load asset loans")
        }
    }
}

/// Loans issued against a ticket. Visibility-gated by `TicketAccess`, so a
/// caller who can't read the ticket can't read its loans.
pub async fn list_for_ticket(mut tc: TenantConn, access: TicketAccess) -> impl Responder {
    let ticket_id = access.ticket_id;
    match tc.run(|conn| repo::list_for_ticket(conn, ticket_id)) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(ticket_id, error = ?e, "failed to list ticket loans");
            errors::internal("Failed to load ticket loans")
        }
    }
}

pub async fn issue(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    body: web::Json<IssueBody>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can issue loaners",
        );
    }
    let asset_id = path.into_inner();
    let body = body.into_inner();
    let input = IssueLoan {
        asset_id,
        borrower_user_uuid: body.borrower_user_uuid,
        loaned_at: body.loaned_at,
        due_back: body.due_back,
        ticket_id: body.ticket_id,
        notes: body.notes,
        actor_uuid: Some(auth.user_uuid),
    };
    match tc.run_result(|conn| repo::issue(conn, input)) {
        Ok(loan) => HttpResponse::Created().json(loan),
        Err(e) => map_loan_err(asset_id, e),
    }
}

pub async fn return_loan(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<(i32, i32)>,
    body: web::Json<ReturnBody>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can return loaners",
        );
    }
    let (asset_id, loan_id) = path.into_inner();
    let body = body.into_inner();
    let returned_at = body.returned_at.unwrap_or_else(Utc::now);
    match tc.run_result(|conn| {
        repo::return_loan(
            conn,
            asset_id,
            loan_id,
            returned_at,
            Some(auth.user_uuid),
            body.notes,
        )
    }) {
        Ok(loan) => HttpResponse::Ok().json(loan),
        Err(e) => map_loan_err(asset_id, e),
    }
}

pub async fn edit(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<(i32, i32)>,
    body: web::Json<EditBody>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: Only technicians and administrators can edit loans");
    }
    let (asset_id, loan_id) = path.into_inner();
    let body = body.into_inner();
    // A present value sets the field; absent leaves it unchanged.
    match tc.run_result(|conn| {
        repo::edit(
            conn,
            asset_id,
            loan_id,
            body.due_back.map(Some),
            body.notes.map(Some),
        )
    }) {
        Ok(loan) => HttpResponse::Ok().json(loan),
        Err(e) => map_loan_err(asset_id, e),
    }
}
