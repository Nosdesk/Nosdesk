use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{ActiveSession, NewActiveSession};
use crate::schema::active_sessions;

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Delete sessions whose `expires_at` is in the past. Intended to be
/// called on a periodic schedule (see `services::scheduler`) so the
/// `active_sessions` table doesn't accrete dead rows indefinitely.
/// Returns the number of rows removed.
pub fn cleanup_expired(conn: &mut DbConnection) -> Result<usize, diesel::result::Error> {
    diesel::delete(
        active_sessions::table.filter(active_sessions::expires_at.lt(Utc::now().naive_utc())),
    )
    .execute(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Create a new active session
pub fn create_session(
    conn: &mut DbConnection,
    new_session: NewActiveSession,
) -> Result<ActiveSession, diesel::result::Error> {
    diesel::insert_into(active_sessions::table)
        .values(&new_session)
        .get_result(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Create a session and enforce the per-user cap, returning the new session
/// and how many stale ones were evicted to make room.
///
/// Eviction is by `last_active` ascending, so the sessions that go are the ones
/// the user has not touched. The new session is inserted first and carries a
/// fresh `last_active`, so it can never evict itself.
///
/// Insert and prune share one transaction. Two simultaneous logins may both
/// prune; because both order by the same column and keep the same newest N,
/// the second is a no-op rather than a conflict.
pub fn create_session_capped(
    conn: &mut DbConnection,
    new_session: NewActiveSession,
) -> Result<(ActiveSession, usize), diesel::result::Error> {
    conn.transaction(|conn| {
        let user_uuid = new_session.user_uuid;
        let session = create_session(conn, new_session)?;

        // The ids to keep: the newest N by activity. Diesel has no DELETE ...
        // USING a subquery of the same table, so select then delete.
        let keep: Vec<i32> = active_sessions::table
            .select(active_sessions::id)
            .filter(active_sessions::user_uuid.eq(user_uuid))
            .order_by(active_sessions::last_active.desc())
            .limit(crate::utils::session_policy::max_sessions_per_user())
            .load(conn)?;

        let evicted = diesel::delete(
            active_sessions::table
                .filter(active_sessions::user_uuid.eq(user_uuid))
                .filter(active_sessions::id.ne_all(keep)),
        )
        .execute(conn)?;

        Ok((session, evicted))
    })
}

/// Get all active sessions for a user
pub fn get_user_sessions(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> Result<Vec<ActiveSession>, diesel::result::Error> {
    active_sessions::table
        .filter(active_sessions::user_uuid.eq(user_uuid))
        .filter(active_sessions::expires_at.gt(Utc::now().naive_utc()))
        .order_by(active_sessions::last_active.desc())
        .load::<ActiveSession>(conn)
}

/// Get a session by its stable UUID
pub fn get_session_by_session_id(
    conn: &mut DbConnection,
    sid: &Uuid,
) -> Result<ActiveSession, diesel::result::Error> {
    active_sessions::table
        .filter(active_sessions::session_id.eq(sid))
        .first::<ActiveSession>(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Revoke a session by its stable UUID (CASCADE deletes linked refresh_tokens)
pub fn revoke_session_by_uuid(
    conn: &mut DbConnection,
    sid: &Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(active_sessions::table.filter(active_sessions::session_id.eq(sid))).execute(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Update session activity timestamp and expiry
pub fn update_session_activity(
    conn: &mut DbConnection,
    sid: &Uuid,
    new_expires_at: chrono::NaiveDateTime,
) -> Result<usize, diesel::result::Error> {
    diesel::update(active_sessions::table.filter(active_sessions::session_id.eq(sid)))
        .set((
            active_sessions::last_active.eq(Utc::now().naive_utc()),
            active_sessions::expires_at.eq(new_expires_at),
        ))
        .execute(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Revoke every session for a user, including the caller's own. Used where the
/// account's credentials just changed out from under all of them (password
/// reset) or the account lost its upstream identity (a removed SSO mapping).
/// To keep the caller signed in, use [`revoke_other_sessions_by_uuid`].
pub fn revoke_all_sessions(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(active_sessions::table.filter(active_sessions::user_uuid.eq(user_uuid)))
        .execute(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Revoke all sessions for a user except the one with the given UUID
pub fn revoke_other_sessions_by_uuid(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    current_session_uuid: &Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(
        active_sessions::table
            .filter(active_sessions::user_uuid.eq(user_uuid))
            .filter(active_sessions::session_id.ne(current_session_uuid)),
    )
    .execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use chrono::Utc;

    fn make_session(user_uuid: Uuid) -> NewActiveSession {
        NewActiveSession {
            user_uuid,
            device_name: None,
            ip_address: None,
            user_agent: Some("test-agent".to_string()),
            location: None,
            expires_at: (Utc::now() + chrono::Duration::hours(1)).naive_utc(),
            oidc_id_token: None,
        }
    }

    #[test]
    fn create_and_get_session() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sessuser", "user");

        let session = create_session(&mut conn, make_session(user.uuid)).unwrap();
        let fetched = get_session_by_session_id(&mut conn, &session.session_id).unwrap();
        assert_eq!(fetched.id, session.id);
    }

    #[test]
    fn revoke_session_by_uuid_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "revokuuid", "user");

        let session = create_session(&mut conn, make_session(user.uuid)).unwrap();
        let rows = revoke_session_by_uuid(&mut conn, &session.session_id).unwrap();
        assert_eq!(rows, 1);
        assert!(get_session_by_session_id(&mut conn, &session.session_id).is_err());
    }

    #[test]
    fn get_user_sessions_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "multiuser", "user");

        create_session(&mut conn, make_session(user.uuid)).unwrap();
        create_session(&mut conn, make_session(user.uuid)).unwrap();

        let sessions = get_user_sessions(&mut conn, &user.uuid).unwrap();
        assert!(sessions.len() >= 2);
    }

    #[test]
    fn update_session_activity_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "actuser", "user");

        let session = create_session(&mut conn, make_session(user.uuid)).unwrap();
        let new_expires = (Utc::now() + chrono::Duration::days(7)).naive_utc();
        let rows = update_session_activity(&mut conn, &session.session_id, new_expires).unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn capped_create_evicts_the_least_recently_active() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "cappeduser", "user");
        let cap = crate::utils::session_policy::max_sessions_per_user() as usize;

        // Fill to the cap, giving each a distinct last_active so "stalest" is
        // unambiguous. The first one created is the one that should go.
        let mut created = Vec::new();
        for i in 0..cap {
            let session = create_session(&mut conn, make_session(user.uuid)).unwrap();
            let backdated = (Utc::now() - chrono::Duration::minutes((cap - i) as i64)).naive_utc();
            diesel::update(active_sessions::table.find(session.id))
                .set(active_sessions::last_active.eq(backdated))
                .execute(&mut conn)
                .unwrap();
            created.push(session);
        }

        let (newest, evicted) = create_session_capped(&mut conn, make_session(user.uuid)).unwrap();

        assert_eq!(evicted, 1, "one over the cap should evict exactly one");
        let remaining = get_user_sessions(&mut conn, &user.uuid).unwrap();
        assert_eq!(remaining.len(), cap);
        // The stalest is gone and the new one survived.
        assert!(get_session_by_session_id(&mut conn, &created[0].session_id).is_err());
        assert!(get_session_by_session_id(&mut conn, &newest.session_id).is_ok());
    }

    #[test]
    fn capped_create_is_a_plain_insert_below_the_cap() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "undercapuser", "user");

        let (first, evicted) = create_session_capped(&mut conn, make_session(user.uuid)).unwrap();
        assert_eq!(evicted, 0);
        assert!(get_session_by_session_id(&mut conn, &first.session_id).is_ok());
    }

    #[test]
    fn cap_is_per_user() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "capmine", "user");
        let other = TestFixtures::create_user(&mut conn, "captheirs", "user");
        let cap = crate::utils::session_policy::max_sessions_per_user() as usize;

        let theirs = create_session(&mut conn, make_session(other.uuid)).unwrap();
        for _ in 0..=cap {
            create_session_capped(&mut conn, make_session(user.uuid)).unwrap();
        }

        // Filling one user's quota must not touch anyone else's sessions.
        assert!(get_session_by_session_id(&mut conn, &theirs.session_id).is_ok());
    }
}
