//! `GET /api/sync/schema` — returns the binary's compiled schema
//! hash so the client can name its IndexedDB instance and detect
//! mismatches before opening a bootstrap stream.
//!
//! Cheap to call; no DB round-trip. Designed for the client to hit
//! once per cold start before `hydrate()` opens IDB.

use actix_web::{HttpResponse, Responder};
use serde::Serialize;

const SERVER_SCHEMA_HASH: &str = env!("NOSDESK_SCHEMA_HASH");

#[derive(Serialize)]
pub struct SchemaResponse {
    pub server_schema: &'static str,
}

pub async fn schema() -> impl Responder {
    HttpResponse::Ok().json(SchemaResponse {
        server_schema: SERVER_SCHEMA_HASH,
    })
}
