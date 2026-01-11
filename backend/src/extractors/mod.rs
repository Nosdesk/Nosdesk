//! Custom Actix extractors for authentication and authorization
//!
//! Provides type-safe extractors that automatically handle auth context.

mod auth_context;

pub use auth_context::AuthContext;
