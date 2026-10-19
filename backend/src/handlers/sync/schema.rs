//! `GET /api/sync/schema` — returns the binary's compiled schema
//! hash, the database's instance id and the request's workspace data
//! epoch, so the client can name its IndexedDB instance, detect schema
//! mismatches before opening a bootstrap stream, and fence its local
//! caches to the current database generation and to the current
//! generation of this workspace's data.
//!
//! Called once per cold start before `hydrate()`. The instance id and
//! the epoch are single indexed PK lookups; on a DB error each comes back
//! empty so the client simply skips that check rather than wiping on a
//! transient failure.

use actix_web::{web, HttpResponse, Responder};
use serde::Serialize;

use crate::db::Pool;
use crate::extractors::WorkspaceContext;

const SERVER_SCHEMA_HASH: &str = env!("NOSDESK_SCHEMA_HASH");

#[derive(Serialize)]
pub struct SchemaResponse {
    pub server_schema: &'static str,
    pub instance_id: String,
    /// The workspace's data epoch (`workspaces.data_epoch`). Changes when
    /// the workspace's data is replaced; the client wipes its caches when it
    /// differs from the one it cached under. `None` when the data has never
    /// been replaced, no workspace is resolved, or the read failed.
    pub workspace_epoch: Option<uuid::Uuid>,
}

pub async fn schema(pool: web::Data<Pool>, ws: Option<WorkspaceContext>) -> impl Responder {
    let mut conn = pool.get().ok();
    let instance_id = conn
        .as_mut()
        .and_then(|conn| crate::sync::system_meta::instance_id(conn).ok())
        .unwrap_or_default();
    let workspace_epoch = match (conn.as_mut(), ws) {
        (Some(conn), Some(ws)) => crate::repository::workspaces::data_fence(conn, ws.workspace_id)
            .ok()
            .and_then(|fence| fence.epoch),
        _ => None,
    };

    HttpResponse::Ok().json(SchemaResponse {
        server_schema: SERVER_SCHEMA_HASH,
        instance_id,
        workspace_epoch,
    })
}
