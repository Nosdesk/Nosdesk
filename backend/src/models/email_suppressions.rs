use diesel::prelude::*;
use serde::{Deserialize, Serialize};

// === Email suppression list ==================================
//
// Addresses on this list are skipped by the outbound enqueue path.
// Auto-populated by hard-bounce detection (J Pass 2.2b) and
// manually managed by admins via the suppression admin view. See
// migration `2026-05-12-110000_email_suppressions`.

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(primary_key(workspace_id, email))]
#[diesel(table_name = crate::schema::email_suppressions)]
pub struct EmailSuppression {
    pub email: String,
    /// Short identifier the admin UI groups on: `hard_bounce`,
    /// `manual`, `complaint`. Kept loose so future categories
    /// (`unsubscribe`, `gdpr_erase`) don't require a migration.
    pub reason: String,
    /// Verbatim upstream diagnostic from the most recent bounce.
    /// `NULL` for manually-added entries.
    pub bounce_diagnostic: Option<String>,
    /// Bumped each time the same address bounces again so admins
    /// can spot chronic vs one-off issues.
    pub bounce_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
    pub metadata: serde_json::Value,
    /// The workspace this suppression belongs to; the list is per relationship.
    pub workspace_id: i32,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::email_suppressions)]
pub struct NewEmailSuppression {
    pub email: String,
    pub reason: String,
    pub bounce_diagnostic: Option<String>,
    /// Suppression is per relationship, not per address: it records that
    /// *this* workspace should stop writing to someone. Required with no
    /// default, so a new write site has to say which workspace it speaks for.
    pub workspace_id: i32,
}

/// Suppression reason constants.
pub mod email_suppression_reason {
    pub const HARD_BOUNCE: &str = "hard_bounce";
    pub const MANUAL: &str = "manual";
    /// Recipient marked a message as spam (a feedback-loop complaint).
    /// Continuing to send to a complainer wrecks sender reputation.
    pub const COMPLAINT: &str = "complaint";
}
