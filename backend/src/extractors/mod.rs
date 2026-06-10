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
