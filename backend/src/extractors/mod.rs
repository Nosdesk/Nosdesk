//! Custom Actix extractors for authentication and authorization
//!
//! Provides type-safe extractors that automatically handle auth context.

pub mod auth_context;
mod platform_auth;
mod platform_conn;
mod scoped_storage;
mod sync_context;
mod tenant_conn;
mod ticket_access;
pub mod workspace_context;

pub use auth_context::AuthContext;
pub use platform_auth::{platform_auth_middleware, PlatformAuth};
#[allow(unused_imports)]
pub use platform_conn::PlatformConn;
pub use scoped_storage::ScopedStorage;
pub use sync_context::SyncContext;
#[allow(unused_imports)]
pub use tenant_conn::TenantConn;
pub use ticket_access::TicketAccess;
pub use workspace_context::WorkspaceContext;

#[cfg(test)]
mod envelope_tests {
    //! Every extractor refusal answers with the API's `{error, code}`
    //! envelope, and every 401 carries a `WWW-Authenticate` challenge.

    use actix_web::body::to_bytes;
    use actix_web::http::StatusCode;
    use actix_web::ResponseError;

    async fn check(err: &dyn ResponseError, status: StatusCode, code: &str) {
        let resp = err.error_response();
        assert_eq!(resp.status(), status, "{err}");
        if status == StatusCode::UNAUTHORIZED {
            assert!(
                resp.headers().contains_key("WWW-Authenticate"),
                "{err}: a 401 carries a challenge"
            );
        }
        let body = to_bytes(resp.into_body()).await.expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body)
            .unwrap_or_else(|_| panic!("{err}: body is not JSON: {body:?}"));
        assert!(json["error"].is_string(), "{err}: {json}");
        assert_eq!(json["code"], code, "{err}: {json}");
    }

    #[actix_web::test]
    async fn extractor_refusals_carry_the_error_envelope() {
        use super::auth_context::AuthContextError as A;
        use super::platform_auth::PlatformAuthError as P;
        use super::platform_conn::PlatformConnError as PC;
        use super::scoped_storage::ScopedStorageError as S;
        use super::sync_context::SyncContextError as Y;
        use super::tenant_conn::TenantConnError as T;
        use super::ticket_access::TicketAccessError as K;
        use super::workspace_context::WorkspaceContextError as W;

        let (bad, unauth, missing, internal) = (
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::NOT_FOUND,
            StatusCode::INTERNAL_SERVER_ERROR,
        );
        check(&A::Unauthorized, unauth, "AUTH_REQUIRED").await;
        check(&A::InvalidUuid, bad, "BAD_REQUEST").await;
        check(&A::UserNotFound, missing, "RESOURCE_NOT_FOUND").await;
        check(&A::DatabaseError("x".into()), internal, "INTERNAL_ERROR").await;
        check(&Y::Unauthorized, unauth, "AUTH_REQUIRED").await;
        check(&Y::InvalidUuid, bad, "BAD_REQUEST").await;
        check(&Y::UserNotFound, missing, "RESOURCE_NOT_FOUND").await;
        check(&Y::DatabaseError("x".into()), internal, "INTERNAL_ERROR").await;
        check(&T::MissingRequestContext, unauth, "AUTH_REQUIRED").await;
        check(&T::PoolError("x".into()), internal, "INTERNAL_ERROR").await;
        check(&T::NoWorkspaceSelected, bad, "NO_WORKSPACE_SELECTED").await;
        check(&K::NoTicketIdInRoute, internal, "INTERNAL_ERROR").await;
        check(&K::BadTicketId, bad, "BAD_REQUEST").await;
        check(&K::Database("x".into()), internal, "INTERNAL_ERROR").await;
        check(&K::NotVisible, missing, "RESOURCE_NOT_FOUND").await;
        check(&K::Auth(A::Unauthorized), unauth, "AUTH_REQUIRED").await;
        check(&S::StorageUnavailable, internal, "INTERNAL_ERROR").await;
        check(&S::NoWorkspace, missing, "RESOURCE_NOT_FOUND").await;
        check(&W::Missing, missing, "RESOURCE_NOT_FOUND").await;
        check(&PC::PoolError("x".into()), internal, "INTERNAL_ERROR").await;
        check(&P::NotHosted, missing, "RESOURCE_NOT_FOUND").await;
        check(&P::Unauthorized, unauth, "AUTH_REQUIRED").await;
    }
}
