//! Workspace-scoped storage extractor.
//!
//! Yields an `Arc<dyn Storage>` whose physical keys are prefixed with the
//! resolved workspace (`ws/{id}/...`) via [`WorkspaceScopedStorage`], so
//! tenant objects a handler writes/reads/moves/deletes are confined to the
//! caller's workspace. Public `/uploads/...` URLs stay logical/stable.
//!
//! Use this in any handler that touches tenant files instead of the raw
//! `web::Data<Arc<dyn Storage>>`; the workspace comes from the
//! `WorkspaceContext` the middleware attaches to every request.

use std::sync::Arc;

use actix_web::{dev::Payload, web, FromRequest, HttpMessage, HttpRequest};
use std::future::{ready, Ready};

use crate::extractors::WorkspaceContext;
use crate::utils::storage::{Storage, WorkspaceScopedStorage};

/// A storage handle scoped to the request's workspace.
pub struct ScopedStorage(pub Arc<dyn Storage>);

impl ScopedStorage {
    /// The scoped `Arc<dyn Storage>`, cloned for passing into async helpers.
    pub fn get(&self) -> Arc<dyn Storage> {
        self.0.clone()
    }
}

#[derive(Debug)]
pub enum ScopedStorageError {
    /// The storage backend wasn't registered as app data.
    StorageUnavailable,
    /// No workspace resolved for the request (apex/unknown host).
    NoWorkspace,
}

impl std::fmt::Display for ScopedStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StorageUnavailable => write!(f, "Storage backend unavailable"),
            Self::NoWorkspace => write!(f, "No workspace context for this request"),
        }
    }
}

impl actix_web::ResponseError for ScopedStorageError {
    fn error_response(&self) -> actix_web::HttpResponse {
        use actix_web::HttpResponse;
        match self {
            Self::StorageUnavailable => HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Storage backend unavailable"})),
            Self::NoWorkspace => {
                HttpResponse::NotFound().json(serde_json::json!({"error": "Workspace not found"}))
            }
        }
    }
}

impl FromRequest for ScopedStorage {
    type Error = ScopedStorageError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let base = match req.app_data::<web::Data<Arc<dyn Storage>>>() {
            Some(data) => data.get_ref().clone(),
            None => return ready(Err(ScopedStorageError::StorageUnavailable)),
        };
        let workspace_id = req
            .extensions()
            .get::<WorkspaceContext>()
            .map(|w| w.workspace_id);
        match workspace_id {
            Some(id) => ready(Ok(ScopedStorage(WorkspaceScopedStorage::arc(base, id)))),
            None => ready(Err(ScopedStorageError::NoWorkspace)),
        }
    }
}
