//! Repository for the inbound dead-letter log.
//!
//! Clean inbound mail that resolved to no active forwarding token lands here
//! (see [`crate::models::InboundDeadLetter`]). The table is platform-level
//! (untenanted), so these queries run on a system/background connection and
//! the read side is gated to the operator at the handler layer, not by RLS.

use diesel::prelude::*;
use diesel::QueryResult;

use crate::db::DbConnection;
use crate::models::{InboundDeadLetter, NewInboundDeadLetter};

// sync-audit-only: operational dead-letter log (a retention/diagnostic table), not a sync aggregate any client subscribes to; untenanted, so there is no workspace sync group to emit into.
/// Record a piece of unrouted-but-clean inbound mail.
pub fn record(
    conn: &mut DbConnection,
    new: NewInboundDeadLetter,
) -> QueryResult<InboundDeadLetter> {
    use crate::schema::inbound_dead_letters::dsl as dl;
    diesel::insert_into(dl::inbound_dead_letters)
        .values(&new)
        .get_result(conn)
}

/// Most recent dead-letter rows, newest first. Drives the operator's
/// "unrouted inbound" view.
pub fn list_recent(conn: &mut DbConnection, limit: i64) -> QueryResult<Vec<InboundDeadLetter>> {
    use crate::schema::inbound_dead_letters::dsl as dl;
    dl::inbound_dead_letters
        .order(dl::received_at.desc())
        .limit(limit)
        .load(conn)
}

/// Count of dead-letter rows received on or after `since`. Drives the
/// "N messages to an unrecognized address in the last 7 days" badge.
pub fn count_since(conn: &mut DbConnection, since: chrono::NaiveDateTime) -> QueryResult<i64> {
    use crate::schema::inbound_dead_letters::dsl as dl;
    dl::inbound_dead_letters
        .filter(dl::received_at.ge(since))
        .count()
        .get_result(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::INBOUND_DEAD_LETTER_REASON_UNKNOWN_TOKEN;

    fn sample(recipient: &str) -> NewInboundDeadLetter {
        NewInboundDeadLetter {
            envelope_recipient: recipient.to_string(),
            from_address: Some("sender@example.com".to_string()),
            subject: Some("hello".to_string()),
            s3_key: "inbound/raw/abc123".to_string(),
            reason: INBOUND_DEAD_LETTER_REASON_UNKNOWN_TOKEN.to_string(),
        }
    }

    #[test]
    fn record_then_list_and_count() {
        let mut conn = crate::test_helpers::setup_test_connection();

        // Epoch is comfortably within PostgreSQL's timestamp range and before
        // any row this test inserts (NaiveDateTime::MIN would overflow it).
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();
        let before = count_since(&mut conn, epoch).unwrap();
        let row = record(&mut conn, sample("deadbeef@inbound.nosdesk.com")).unwrap();
        assert_eq!(row.reason, INBOUND_DEAD_LETTER_REASON_UNKNOWN_TOKEN);
        assert_eq!(row.from_address.as_deref(), Some("sender@example.com"));

        let recent = list_recent(&mut conn, 10).unwrap();
        assert!(recent.iter().any(|r| r.id == row.id));

        let after = count_since(&mut conn, epoch).unwrap();
        assert_eq!(after, before + 1);
    }

    #[test]
    fn list_recent_respects_limit() {
        let mut conn = crate::test_helpers::setup_test_connection();
        for i in 0..3 {
            record(&mut conn, sample(&format!("t{i}@inbound.nosdesk.com"))).unwrap();
        }
        let limited = list_recent(&mut conn, 2).unwrap();
        assert_eq!(limited.len(), 2);
    }
}
