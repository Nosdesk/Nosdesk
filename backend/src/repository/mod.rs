// Domain-specific modules
pub mod analytics;
pub mod article_content;
pub mod asset_audits;
pub mod asset_groups;
pub mod asset_kinds;
pub mod asset_lifecycle;
pub mod asset_loans;
pub mod asset_media;
pub mod asset_models;
pub mod asset_usage;
pub mod assets;
pub mod assignment_rules;
pub mod audit;
pub mod audit_log;
pub mod bug_reports;
pub mod canned_responses;
pub mod categories;
pub mod channels;
pub mod comments;
pub mod cycles;
pub mod dashboard_stats;
pub mod directory;
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
pub mod inbound_addresses;
pub mod inbound_dead_letters;
pub mod instance_settings;
pub mod knowledge_gaps;
pub mod linked_tickets;
pub mod manufacturers;
pub mod outbound_emails;
pub mod passkey_credentials;
pub mod projects;
pub mod push_devices;
pub mod requester_identities;
pub mod rules;
pub mod saved_views;
pub mod search_query_log;
pub mod sla;
pub mod sla_admin;
pub mod sync_history;
pub mod tags;
pub mod ticket_approvals;
pub mod ticket_merge;
pub mod ticket_query;
pub mod ticket_ratings;
pub mod ticket_visibility;
pub mod ticket_watchers;
pub mod tickets;
pub mod user_auth_identities;
pub mod user_contact;
pub mod user_emails;
pub mod user_helpers; // Helper functions for user/email operations
pub mod user_locale;
pub mod user_preferences;
pub mod user_profile;
pub mod user_recovery_codes;
pub mod users;
pub mod widget_visitors;
pub mod workflow_states;
pub mod workspace_file_purges;
pub mod workspace_identity_providers;
pub mod workspace_notices;
pub mod workspace_widget_settings;
pub mod workspaces;
pub mod yjs_snapshots;

// Security and session management repositories
pub mod active_sessions;
pub mod api_tokens;
pub mod refresh_tokens;
pub mod reset_tokens;
pub mod user_ticket_views;

// Site configuration
pub mod site_settings;

// Per-workspace outbound email identity
pub mod workspace_activation;
pub mod workspace_email_settings;
pub mod workspace_export_jobs;
pub mod workspace_ldap_settings;

// Backup and restore
pub mod backup;

// Webhooks
pub mod webhooks;

// CSP violation reports
pub mod csp_reports;
pub mod guest_residue;

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

/// The workspace the connection is pinned to (`app.workspace_id`), or NULL
/// when nothing is pinned. Tenant tables default `workspace_id` from the same
/// setting, so filtering on it keeps an elevated (BYPASSRLS) caller's reads in
/// the workspace its inserts land in, and gives an unpinned connection nothing.
pub(crate) fn pinned_workspace() -> diesel::expression::SqlLiteral<diesel::sql_types::Integer> {
    diesel::dsl::sql::<diesel::sql_types::Integer>(
        "NULLIF(current_setting('app.workspace_id', true), '')::int",
    )
}

/// `s` as an `ILIKE` pattern that matches only itself: `%`, `_` and `\`
/// escaped.
pub(crate) fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

// Note: We've completed the transition to a fully modular structure
// by removing the base.rs file and keeping only domain-specific modules.

#[cfg(test)]
mod tests {
    use super::escape_like;

    #[test]
    fn escape_like_escapes_all_three_metacharacters() {
        assert_eq!(escape_like("50%"), r"50\%");
        assert_eq!(escape_like("some_name"), r"some\_name");
        // Backslash must be escaped first; otherwise we'd double-escape % / _.
        assert_eq!(escape_like(r"C:\path"), r"C:\\path");
        assert_eq!(escape_like(r"a\%b_c"), r"a\\\%b\_c");
    }

    #[test]
    fn escape_like_passes_through_safe_input() {
        assert_eq!(escape_like("hello world"), "hello world");
        assert_eq!(escape_like(""), "");
    }
}
