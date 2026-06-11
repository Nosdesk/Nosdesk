//! `GET /api/sync/schema` — returns the binary's compiled schema
//! hash plus the database's instance id, so the client can name its
//! IndexedDB instance, detect schema mismatches before opening a
//! bootstrap stream, and fence its local caches to the current
//! database generation (see docs/plans/collab-stale-cache-fence.md).
//!
//! Called once per cold start before `hydrate()`. The instance id is a
//! single indexed PK lookup; on a DB error we return an empty string so
//! the client simply skips the epoch check rather than wiping on a
//! transient failure.

use actix_web::{web, HttpResponse, Responder};
use serde::Serialize;

use crate::db::Pool;

const SERVER_SCHEMA_HASH: &str = env!("NOSDESK_SCHEMA_HASH");

#[derive(Serialize)]
pub struct SchemaResponse {
    pub server_schema: &'static str,
    pub instance_id: String,
}

pub async fn schema(pool: web::Data<Pool>) -> impl Responder {
    let instance_id = pool
        .get()
        .ok()
        .and_then(|mut conn| crate::sync::system_meta::instance_id(&mut conn).ok())
        .unwrap_or_default();

    HttpResponse::Ok().json(SchemaResponse {
        server_schema: SERVER_SCHEMA_HASH,
        instance_id,
    })
}
