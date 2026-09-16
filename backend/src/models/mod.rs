//! Model types, one file per domain.
//!
//! Placement rule: a file here is named for a module that already exists
//! in `handlers/` or `repository/`, never a new name. A type goes in the
//! coarsest such file it would be searched under; a row that only its
//! repository touches takes that repository's name. Rows and API shapes
//! for the same noun live together.
//!
//! Every file is re-exported flat, so `crate::models::Ticket` is the path
//! regardless of which file holds it.

mod active_sessions;
mod api_tokens;
mod article_content;
mod asset_groups;
mod assets;
mod assignment_rules;
mod audit_log;
mod auth;
mod backup;
mod bug_reports;
mod canned_responses;
mod categories;
mod channels;
mod comments;
mod csp_reports;
mod cycles;
mod documentation;
mod documentation_collections;
mod email_suppressions;
mod groups;
mod idempotency_keys;
mod imports;
mod knowledge_gaps;
mod notifications;
mod outbound_emails;
mod passkey_credentials;
mod plugin_collections;
mod plugin_manifest;
mod plugins;
mod projects;
mod refresh_tokens;
mod reset_tokens;
mod rules;
mod saved_views;
mod search_query_log;
mod security_events;
mod site_settings;
mod sla;
mod sync_actions;
mod sync_history;
mod tags;
mod tickets;
mod user_auth_identities;
mod user_contact;
mod user_recovery_codes;
mod user_ticket_views;
mod users;
mod webhooks;
mod workflow_states;
mod workspace_export_jobs;
mod workspace_ldap_settings;
mod workspaces;

pub use active_sessions::*;
pub use api_tokens::*;
pub use article_content::*;
pub use asset_groups::*;
pub use assets::*;
pub use assignment_rules::*;
pub use audit_log::*;
pub use auth::*;
pub use backup::*;
pub use bug_reports::*;
pub use canned_responses::*;
pub use categories::*;
pub use channels::*;
pub use comments::*;
pub use csp_reports::*;
pub use cycles::*;
pub use documentation::*;
pub use documentation_collections::*;
pub use email_suppressions::*;
pub use groups::*;
pub use idempotency_keys::*;
pub use imports::*;
pub use knowledge_gaps::*;
pub use notifications::*;
pub use outbound_emails::*;
pub use passkey_credentials::*;
pub use plugin_collections::*;
pub use plugin_manifest::*;
pub use plugins::*;
pub use projects::*;
pub use refresh_tokens::*;
pub use reset_tokens::*;
pub use rules::*;
pub use saved_views::*;
pub use search_query_log::*;
pub use security_events::*;
pub use site_settings::*;
pub use sla::*;
pub use sync_actions::*;
pub use sync_history::*;
pub use tags::*;
pub use tickets::*;
pub use user_auth_identities::*;
pub use user_contact::*;
pub use user_recovery_codes::*;
pub use user_ticket_views::*;
pub use users::*;
pub use webhooks::*;
pub use workflow_states::*;
pub use workspace_export_jobs::*;
pub use workspace_ldap_settings::*;
pub use workspaces::*;

use uuid::Uuid;

// Removed unused import: use diesel::sql_types::Text;

// Simple UUID serialization helpers
fn serialize_optional_uuid_as_string<S>(
    uuid: &Option<Uuid>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&uuid.map(|u| u.to_string()).unwrap_or_default())
}
