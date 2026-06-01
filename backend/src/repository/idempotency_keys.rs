//! Idempotency-Key cache for the workspace-provisioning callback
//! endpoints (M5 Task 2). Stripe-style: an `Idempotency-Key` header
//! on a POST / PUT / PATCH is checked against this table; on a hit,
//! the middleware short-circuits with the cached response so retries
//! don't double-execute the handler.
//!
//! Race note: two concurrent requests with the same key can both
//! miss the lookup, both run the handler, and both try to insert.
//! `try_insert` uses `ON CONFLICT (key) DO NOTHING`, so the second
//! insert is a no-op — but the second caller does see the second
//! handler's response body, not the cached one. That's tolerable
//! here because the M5 control-plane worker is single-threaded per
//! provisioning attempt; if we ever ship a concurrent retry path
//! we'd need to switch to a "INSERT pending row first, lock, then
//! update with result" pattern.

use chrono::NaiveDateTime;
use diesel::prelude::*;
use diesel::result::Error;
use serde_json::Value as JsonValue;

use crate::db::DbConnection;
use crate::models::{IdempotencyRecord, NewIdempotencyRecord};
use crate::schema::idempotency_keys;

// sync-audit-only: platform-level cache for retry idempotency; never workspace-scoped, never observed by app clients

/// Look up a cached response. Returns `Ok(None)` if the key hasn't
/// been seen yet.
pub fn try_get(conn: &mut DbConnection, key: &str) -> Result<Option<IdempotencyRecord>, Error> {
    idempotency_keys::table
        .filter(idempotency_keys::key.eq(key))
        .first::<IdempotencyRecord>(conn)
        .optional()
}

// sync-audit-only: platform-level cache for retry idempotency; never workspace-scoped, never observed by app clients
/// Cache a fresh response. `ON CONFLICT DO NOTHING` so a concurrent
/// retry race doesn't trip the PK constraint (see module note).
/// Returns the row that ended up in the table — caller's row if the
/// insert won the race, the prior row if it lost.
pub fn upsert(
    conn: &mut DbConnection,
    key: &str,
    response_status: i16,
    response_body: &JsonValue,
) -> Result<IdempotencyRecord, Error> {
    let new = NewIdempotencyRecord {
        key: key.to_string(),
        response_status,
        response_body: response_body.clone(),
    };
    diesel::insert_into(idempotency_keys::table)
        .values(&new)
        .on_conflict(idempotency_keys::key)
        .do_nothing()
        .execute(conn)?;
    // Re-read to pick up either our own insert or the racer's row.
    idempotency_keys::table
        .filter(idempotency_keys::key.eq(key))
        .first(conn)
}

// sync-audit-only: platform-level cache for retry idempotency; never workspace-scoped, never observed by app clients
/// Drop rows past the retention horizon. Returns rowcount for logging.
/// Caller supplies the horizon as a `NaiveDateTime` so tests can pin
/// the boundary; production uses `chrono::Utc::now() - 24h`.
pub fn prune_older_than(
    conn: &mut DbConnection,
    older_than: NaiveDateTime,
) -> Result<usize, Error> {
    diesel::delete(idempotency_keys::table.filter(idempotency_keys::created_at.lt(older_than)))
        .execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;
    use chrono::Duration as ChronoDuration;
    use serde_json::json;

    #[test]
    fn try_get_returns_none_for_unknown_key() {
        let mut conn = setup_test_connection();
        assert!(try_get(&mut conn, "missing").unwrap().is_none());
    }

    #[test]
    fn upsert_then_get_round_trips_body_and_status() {
        let mut conn = setup_test_connection();
        let body = json!({ "workspace_uuid": "abc", "slug": "acme" });
        upsert(&mut conn, "workspaces-create:abc", 201, &body).unwrap();
        let row = try_get(&mut conn, "workspaces-create:abc")
            .unwrap()
            .unwrap();
        assert_eq!(row.response_status, 201);
        assert_eq!(row.response_body, body);
    }

    #[test]
    fn upsert_is_idempotent_returns_first_writers_row() {
        let mut conn = setup_test_connection();
        let body_a = json!({ "v": 1 });
        let body_b = json!({ "v": 2 });
        let first = upsert(&mut conn, "race:1", 200, &body_a).unwrap();
        let second = upsert(&mut conn, "race:1", 200, &body_b).unwrap();
        // ON CONFLICT DO NOTHING means the first insert wins; second
        // caller observes the cached body, not its own attempt.
        assert_eq!(first.response_body, body_a);
        assert_eq!(second.response_body, body_a);
    }

    #[test]
    fn prune_drops_rows_past_horizon() {
        let mut conn = setup_test_connection();
        // Seed two rows, then backdate one past the horizon by
        // updating created_at directly.
        upsert(&mut conn, "old", 200, &json!({})).unwrap();
        upsert(&mut conn, "fresh", 200, &json!({})).unwrap();
        let backdate = chrono::Utc::now().naive_utc() - ChronoDuration::days(2);
        diesel::update(idempotency_keys::table.filter(idempotency_keys::key.eq("old")))
            .set(idempotency_keys::created_at.eq(backdate))
            .execute(&mut conn)
            .unwrap();

        let horizon = chrono::Utc::now().naive_utc() - ChronoDuration::hours(24);
        let removed = prune_older_than(&mut conn, horizon).unwrap();
        assert_eq!(removed, 1);
        assert!(try_get(&mut conn, "old").unwrap().is_none());
        assert!(try_get(&mut conn, "fresh").unwrap().is_some());
    }
}
