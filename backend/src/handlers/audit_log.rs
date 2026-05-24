//! Admin-only audit-log read API.
//!
//! Single endpoint: `GET /api/admin/audit-log`. Filters by entity
//! (`table_name` + `pk_text`), by actor, and/or by time range.
//! Pagination is keyset-cursor on `(occurred_at DESC, id DESC)`; the
//! cursor is opaque base64-JSON to keep the URL surface tidy.
//!
//! Each row in the response carries a flattened `diff` so the
//! frontend doesn't need to know the trigger schema.

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::repository::audit_log as repo;
use crate::utils::rbac;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub table_name: Option<String>,
    pub pk_text: Option<String>,
    pub actor_uuid: Option<Uuid>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
    /// Opaque cursor from a previous page's `next_cursor` field.
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub rows: Vec<RowResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RowResponse {
    pub id: i64,
    pub table_name: String,
    pub pk_text: String,
    pub op: String,
    pub actor_uuid: Option<Uuid>,
    pub correlation_id: Option<Uuid>,
    pub occurred_at: DateTime<Utc>,
    pub diff: Vec<repo::DiffEntry>,
}

/// `GET /api/admin/audit-log` — admin-gated. See module docs.
pub async fn list(
    req: HttpRequest,
    mut tc: TenantConn,
    query: web::Query<ListQuery>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }

    let cursor = match query.cursor.as_deref().map(decode_cursor) {
        Some(Ok(c)) => Some(c),
        Some(Err(_)) => {
            return errors::bad_request(
                "Invalid cursor; pass the next_cursor from the previous response verbatim",
            );
        }
        None => None,
    };

    // Defaults: last 7 days, 50 rows per page. The trigger fires on every
    // write to audited tables so an unfiltered scan is the wrong default;
    // the time bound keeps sensible queries cheap on partitioned tables.
    let since = query
        .since
        .or_else(|| Some(Utc::now() - chrono::Duration::days(7)));

    let filter = repo::AuditLogFilter {
        table_name: query.table_name.clone(),
        pk_text: query.pk_text.clone(),
        actor_uuid: query.actor_uuid,
        since,
        until: query.until,
    };

    let limit = query.limit.unwrap_or(50);
    let page = match tc.run(|conn| repo::list(conn, &filter, cursor, limit)) {
        Ok(p) => p,
        Err(e) => {
            warn!(error = ?e, "Failed to list audit log");
            return errors::internal("Failed to list audit log");
        }
    };

    let rows = page
        .rows
        .into_iter()
        .map(|row| {
            let diff = repo::flatten_diff(&row);
            RowResponse {
                id: row.id,
                table_name: row.table_name,
                pk_text: row.pk_text,
                op: row.op,
                actor_uuid: row.actor_uuid,
                correlation_id: row.correlation_id,
                occurred_at: row.occurred_at,
                diff,
            }
        })
        .collect();

    HttpResponse::Ok().json(ListResponse {
        rows,
        next_cursor: page.next_cursor.map(encode_cursor),
    })
}

fn encode_cursor(c: repo::Cursor) -> String {
    let json = serde_json::to_vec(&c).expect("Cursor serialises to JSON");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
}

fn decode_cursor(s: &str) -> Result<repo::Cursor, ()> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .map_err(|_| ())?;
    serde_json::from_slice(&bytes).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{claims_for, setup_test_pool};
    use actix_web::test as actix_test;
    use actix_web::{http::StatusCode, App, HttpMessage};

    #[test]
    fn cursor_round_trips() {
        let c = repo::Cursor {
            occurred_at: Utc::now(),
            id: 12345,
        };
        let encoded = encode_cursor(c);
        let decoded = decode_cursor(&encoded).expect("decoded");
        assert_eq!(decoded.id, c.id);
        assert_eq!(
            decoded.occurred_at.timestamp_micros(),
            c.occurred_at.timestamp_micros()
        );
    }

    #[test]
    fn decode_cursor_rejects_garbage() {
        assert!(decode_cursor("not base64!!!").is_err());
        assert!(decode_cursor("aGVsbG8").is_err()); // valid base64, not JSON cursor
    }

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
            .route("/admin/audit-log", web::get().to(list))
    }

    #[actix_web::test]
    async fn audit_log_requires_authentication() {
        let pool = setup_test_pool();
        let app = actix_test::init_service(test_app(pool)).await;

        let req = actix_test::TestRequest::get()
            .uri("/admin/audit-log")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn audit_log_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::User);
        let app = actix_test::init_service(test_app(pool.clone())).await;

        let req = actix_test::TestRequest::get()
            .uri("/admin/audit-log")
            .to_request();
        req.extensions_mut().insert(claims);

        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn audit_log_rejects_technician_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::Technician);
        let app = actix_test::init_service(test_app(pool.clone())).await;

        let req = actix_test::TestRequest::get()
            .uri("/admin/audit-log")
            .to_request();
        req.extensions_mut().insert(claims);

        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn audit_log_admin_succeeds() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::Admin);
        let app = actix_test::init_service(test_app(pool.clone())).await;

        let req = actix_test::TestRequest::get()
            .uri("/admin/audit-log")
            .to_request();
        req.extensions_mut().insert(claims);

        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
