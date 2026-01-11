// Domain-specific modules
pub mod article_content;
pub mod assignment_rules;
pub mod categories;
pub mod comments;
pub mod devices;
pub mod documentation;
pub mod groups;
pub mod linked_tickets;
pub mod projects;
pub mod sync_history;
pub mod ticket_query;
pub mod tickets;
pub mod user_auth_identities;
pub mod user_emails;
pub mod user_helpers; // Helper functions for user/email operations
pub mod users;

// Security and session management repositories
pub mod active_sessions;
pub mod api_tokens;
pub mod refresh_tokens;
pub mod reset_tokens;
pub mod user_ticket_views;

// Site configuration
pub mod site_settings;

// Backup and restore
pub mod backup;

// Re-export all functions
pub use article_content::*;
pub use comments::*;
pub use devices::*;
pub use documentation::*;
pub use linked_tickets::*;
pub use projects::*;
pub use tickets::*;
pub use users::*;

// Note: We've completed the transition to a fully modular structure
// by removing the base.rs file and keeping only domain-specific modules.