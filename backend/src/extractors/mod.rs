//! Custom Actix extractors for authentication and authorization
//!
//! Provides type-safe extractors that automatically handle auth context.

pub mod auth_context;
mod sync_context;
mod ticket_access;
pub mod workspace_context;

pub use auth_context::AuthContext;
pub use sync_context::SyncContext;
pub use ticket_access::TicketAccess;
pub use workspace_context::WorkspaceContext;
