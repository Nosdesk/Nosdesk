//! Operator view of the inbound dead-letter log.
//!
//! `GET /api/admin/inbound/dead-letters` — platform-admin only. The dead-letter
//! table is untenanted (mail to an unknown forwarding token can't be attributed
//! to a workspace), so this is cross-tenant operator data, gated on
//! `require_platform_admin` and read on a system connection. The internal S3
//! key is not exposed; the operator only needs the envelope recipient, sender,
//! subject, and time to spot a misconfigured forward.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Serialize;
use tracing::error;

use crate::db::Pool;
use crate::handlers::errors;
use crate::models::InboundDeadLetter;
use crate::repository::inbound_dead_letters as repo;
use crate::utils::rbac;

/// One unrouted-mail row, minus the internal S3 key.
#[derive(Debug, Serialize)]
pub struct DeadLetterRow {
    pub id: i64,
    pub envelope_recipient: String,
    pub from_address: Option<String>,
    pub subject: Option<String>,
    pub received_at: chrono::NaiveDateTime,
}

impl From<InboundDeadLetter> for DeadLetterRow {
    fn from(r: InboundDeadLetter) -> Self {
        Self {
            id: r.id,
            envelope_recipient: r.envelope_recipient,
            from_address: r.from_address,
            subject: r.subject,
            received_at: r.received_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub rows: Vec<DeadLetterRow>,
    /// Count received in the last 7 days, for the "N unrouted messages" badge.
    pub count_7d: i64,
}

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

/// `GET /api/admin/inbound/dead-letters` — platform-admin only.
pub async fn list(req: HttpRequest, pool: web::Data<Pool>) -> HttpResponse {
    if let Err(resp) = rbac::require_platform_admin(&req) {
        return resp;
    }

    let since = chrono::Utc::now().naive_utc() - chrono::Duration::days(7);
    // cross-tenant: inbound_dead_letters is an untenanted platform-admin table.
    let result = crate::sync::session::background_run(&pool, "inbound:list_dead_letters", |conn| {
        let rows = repo::list_recent(conn, DEFAULT_LIMIT.min(MAX_LIMIT))?;
        let count_7d = repo::count_since(conn, since)?;
        Ok((rows, count_7d))
    });

    match result {
        Ok((rows, count_7d)) => HttpResponse::Ok().json(ListResponse {
            rows: rows.into_iter().map(DeadLetterRow::from).collect(),
            count_7d,
        }),
        Err(e) => {
            error!(error = %e, "failed to list inbound dead-letters");
            errors::internal("Failed to list unrouted inbound mail")
        }
    }
}
