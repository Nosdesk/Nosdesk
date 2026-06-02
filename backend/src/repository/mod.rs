// Domain-specific modules
pub mod article_content;
pub mod asset_audits;
pub mod asset_kinds;
pub mod asset_usage;
pub mod assets;
pub mod assignment_rules;
pub mod audit;
pub mod audit_log;
pub mod canned_responses;
pub mod categories;
pub mod channels;
pub mod comments;
pub mod cycles;
pub mod dashboard_stats;
pub mod documentation;
pub mod documentation_collections;
pub mod documentation_page_tickets;
pub mod documentation_starred_pages;
pub mod documentation_subscriptions;
pub mod email_suppressions;
pub mod feature_flags;
pub mod groups;
pub mod idempotency_keys;
pub mod imports;
pub mod knowledge_gaps;
pub mod linked_tickets;
pub mod outbound_emails;
pub mod passkey_credentials;
pub mod projects;
pub mod saved_views;
pub mod search_query_log;
pub mod sla;
pub mod sla_admin;
pub mod sync_history;
pub mod tags;
pub mod ticket_merge;
pub mod ticket_query;
pub mod ticket_visibility;
pub mod ticket_watchers;
pub mod tickets;
pub mod user_auth_identities;
pub mod user_emails;
pub mod user_helpers; // Helper functions for user/email operations
pub mod user_locale;
pub mod user_preferences;
pub mod user_profile;
pub mod user_recovery_codes;
pub mod users;
pub mod workflow_states;
pub mod workspaces;

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

// Webhooks
pub mod webhooks;

// CSP violation reports
pub mod csp_reports;

// Plugins
pub mod plugin_collections;
pub mod plugin_publishers;
pub mod plugins;

// Re-export all functions
pub use article_content::*;
pub use assets::*;
pub use comments::*;
pub use documentation::*;
pub use linked_tickets::*;
pub use projects::*;
pub use tickets::*;
pub use users::*;

// Note: We've completed the transition to a fully modular structure
// by removing the base.rs file and keeping only domain-specific modules.
