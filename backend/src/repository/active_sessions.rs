use diesel::prelude::*;
use uuid::Uuid;
use chrono::Utc;

use crate::db::DbConnection;
use crate::models::{ActiveSession, NewActiveSession};
use crate::schema::active_sessions;

/// Create a new active session
pub fn create_session(
    conn: &mut DbConnection,
    new_session: NewActiveSession,
) -> Result<ActiveSession, diesel::result::Error> {
    diesel::insert_into(active_sessions::table)
        .values(&new_session)
        .get_result(conn)
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

/// Get a session by its integer primary key (for revoke_session ownership check)
pub fn get_session_by_id(
    conn: &mut DbConnection,
    id: i32,
) -> Result<ActiveSession, diesel::result::Error> {
    active_sessions::table.find(id).first::<ActiveSession>(conn)
}

/// Revoke a specific session by integer ID
pub fn revoke_session(
    conn: &mut DbConnection,
    session_id: i32,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(active_sessions::table.find(session_id))
        .execute(conn)
}

/// Revoke a session by its stable UUID (CASCADE deletes linked refresh_tokens)
pub fn revoke_session_by_uuid(
    conn: &mut DbConnection,
    sid: &Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(
        active_sessions::table.filter(active_sessions::session_id.eq(sid))
    )
    .execute(conn)
}

/// Update session activity timestamp and expiry
pub fn update_session_activity(
    conn: &mut DbConnection,
    sid: &Uuid,
    new_expires_at: chrono::NaiveDateTime,
) -> Result<usize, diesel::result::Error> {
    diesel::update(
        active_sessions::table.filter(active_sessions::session_id.eq(sid))
    )
    .set((
        active_sessions::last_active.eq(Utc::now().naive_utc()),
        active_sessions::expires_at.eq(new_expires_at),
    ))
    .execute(conn)
}

/// Revoke all sessions for a user except the current one
pub fn revoke_other_sessions(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    current_session_id: Option<i32>,
) -> Result<usize, diesel::result::Error> {
    match current_session_id {
        Some(session_id) => {
            diesel::delete(
                active_sessions::table
                    .filter(active_sessions::user_uuid.eq(user_uuid))
                    .filter(active_sessions::id.ne(session_id))
            )
            .execute(conn)
        }
        None => {
            diesel::delete(
                active_sessions::table.filter(active_sessions::user_uuid.eq(user_uuid))
            )
            .execute(conn)
        }
    }
}

/// Revoke all sessions for a user except the one with the given UUID
pub fn revoke_other_sessions_by_uuid(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    current_session_uuid: &Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(
        active_sessions::table
            .filter(active_sessions::user_uuid.eq(user_uuid))
            .filter(active_sessions::session_id.ne(current_session_uuid))
    )
    .execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
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
            is_current: false,
        }
    }

    #[test]
    fn create_and_get_session() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sessuser", UserRole::User);

        let session = create_session(&mut conn, make_session(user.uuid)).unwrap();
        let fetched = get_session_by_session_id(&mut conn, &session.session_id).unwrap();
        assert_eq!(fetched.id, session.id);
    }

    #[test]
    fn revoke_session_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "revokuser", UserRole::User);

        let session = create_session(&mut conn, make_session(user.uuid)).unwrap();
        let rows = revoke_session(&mut conn, session.id).unwrap();
        assert_eq!(rows, 1);
        assert!(get_session_by_session_id(&mut conn, &session.session_id).is_err());
    }

    #[test]
    fn revoke_session_by_uuid_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "revokuuid", UserRole::User);

        let session = create_session(&mut conn, make_session(user.uuid)).unwrap();
        let rows = revoke_session_by_uuid(&mut conn, &session.session_id).unwrap();
        assert_eq!(rows, 1);
        assert!(get_session_by_session_id(&mut conn, &session.session_id).is_err());
    }

    #[test]
    fn get_user_sessions_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "multiuser", UserRole::User);

        create_session(&mut conn, make_session(user.uuid)).unwrap();
        create_session(&mut conn, make_session(user.uuid)).unwrap();

        let sessions = get_user_sessions(&mut conn, &user.uuid).unwrap();
        assert!(sessions.len() >= 2);
    }

    #[test]
    fn update_session_activity_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "actuser", UserRole::User);

        let session = create_session(&mut conn, make_session(user.uuid)).unwrap();
        let new_expires = (Utc::now() + chrono::Duration::days(7)).naive_utc();
        let rows = update_session_activity(&mut conn, &session.session_id, new_expires).unwrap();
        assert_eq!(rows, 1);
    }
}
