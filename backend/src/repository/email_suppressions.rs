//! Email suppression-list repository.
//!
//! Backs J Pass 2.2b: addresses that bounced hard or were
//! manually blocked are stored here, and the outbound enqueue
//! path consults this table before scheduling a send.
//!
//! All writes canonicalise the email to lower-case so that
//! `Alice@Example.com` and `alice@example.com` hash to the same
//! row. The table's primary key is the lower-cased address so
//! re-inserting bumps `bounce_count` and `last_seen_at` via
//! `ON CONFLICT DO UPDATE` rather than failing.

use chrono::{DateTime, Utc};
use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{EmailSuppression, NewEmailSuppression};
use crate::schema::email_suppressions;

// sync-audit-only: suppression list mutation; operational table for email deliverability
/// Insert a suppression for `email` (case-folded). If a row already
/// exists, bump `bounce_count` and refresh `last_seen_at` /
/// `bounce_diagnostic`. The lower-case fold means we treat email
/// addresses as case-insensitive identifiers, matching how almost
/// every receiving MTA actually behaves in 2026.
pub fn upsert(
    conn: &mut DbConnection,
    new: NewEmailSuppression,
) -> Result<EmailSuppression, diesel::result::Error> {
    let normalized = NewEmailSuppression {
        email: new.email.trim().to_ascii_lowercase(),
        ..new
    };
    diesel::insert_into(email_suppressions::table)
        .values(&normalized)
        .on_conflict(email_suppressions::email)
        .do_update()
        .set((
            email_suppressions::bounce_count.eq(email_suppressions::bounce_count + 1),
            email_suppressions::last_seen_at.eq(diesel::dsl::now),
            email_suppressions::bounce_diagnostic.eq(&normalized.bounce_diagnostic),
        ))
        .get_result(conn)
}

/// Is the given address on the suppression list? Case-insensitive
/// match via lower-case fold (mirrors `upsert`). The caller uses
/// this on the outbound enqueue hot path, so it's a single keyed
/// lookup rather than a list scan.
pub fn is_suppressed(conn: &mut DbConnection, email: &str) -> Result<bool, diesel::result::Error> {
    use diesel::dsl::exists;
    use diesel::select;
    let normalized = email.trim().to_ascii_lowercase();
    select(exists(
        email_suppressions::table.filter(email_suppressions::email.eq(&normalized)),
    ))
    .get_result(conn)
}

/// Paginated list for the admin view, newest first.
pub fn list(
    conn: &mut DbConnection,
    limit: i64,
    before: Option<DateTime<Utc>>,
) -> Result<Vec<EmailSuppression>, diesel::result::Error> {
    let mut q = email_suppressions::table
        .order(email_suppressions::created_at.desc())
        .limit(limit)
        .into_boxed();
    if let Some(b) = before {
        q = q.filter(email_suppressions::created_at.lt(b));
    }
    q.load(conn)
}

/// Total count for the admin stats card.
pub fn count(conn: &mut DbConnection) -> Result<i64, diesel::result::Error> {
    email_suppressions::table.count().get_result(conn)
}

// sync-audit-only: suppression list mutation; operational table for email deliverability
/// Remove a suppression. Returns the number of rows deleted so the
/// admin handler can disambiguate "address not on the list" from
/// "removed successfully" without a separate existence check.
pub fn remove(conn: &mut DbConnection, email: &str) -> Result<usize, diesel::result::Error> {
    let normalized = email.trim().to_ascii_lowercase();
    diesel::delete(email_suppressions::table.filter(email_suppressions::email.eq(&normalized)))
        .execute(conn)
}
