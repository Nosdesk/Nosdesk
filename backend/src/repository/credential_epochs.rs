//! How many times each account has been signed out everywhere (see the
//! `credential_epochs` migration). Stateless bearers that outlive a session
//! sign the value they were issued under; [`raise`] runs with every
//! revocation of all of an account's sessions, so those bearers stop working
//! with them.

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::schema::credential_epochs;

/// The account's current epoch: 0 until it is first raised.
pub fn current(conn: &mut DbConnection, user_uuid: &Uuid) -> QueryResult<i32> {
    Ok(credential_epochs::table
        .find(user_uuid)
        .select(credential_epochs::epoch)
        .first(conn)
        .optional()?
        .unwrap_or(0))
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Raise the account's epoch by one, returning the new value.
pub fn raise(conn: &mut DbConnection, user_uuid: &Uuid) -> QueryResult<i32> {
    diesel::insert_into(credential_epochs::table)
        .values((
            credential_epochs::user_uuid.eq(user_uuid),
            credential_epochs::epoch.eq(1),
        ))
        .on_conflict(credential_epochs::user_uuid)
        .do_update()
        .set((
            credential_epochs::epoch.eq(credential_epochs::epoch + 1),
            credential_epochs::raised_at.eq(diesel::dsl::now),
        ))
        .returning(credential_epochs::epoch)
        .get_result(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{active_sessions, reset_tokens};
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn signing_out_everywhere_raises_the_epoch_and_spends_sign_in_links() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "Epoch Requester", "user");
        let magic = crate::utils::reset_tokens::TokenType::PortalMagicLink.as_str();
        let live = |conn: &mut DbConnection| {
            reset_tokens::live_tokens(conn, user.uuid, magic)
                .expect("live tokens")
                .len()
        };
        assert_eq!(current(&mut conn, &user.uuid).expect("epoch"), 0);
        reset_tokens::create_reset_token(
            &mut conn,
            "epoch-test-hash",
            user.uuid,
            magic,
            None,
            None,
            chrono::Utc::now() + chrono::Duration::minutes(15),
            None,
        )
        .expect("sign-in link");
        assert_eq!(live(&mut conn), 1);

        active_sessions::revoke_all_sessions(&mut conn, &user.uuid).expect("revoke all");
        assert_eq!(current(&mut conn, &user.uuid).expect("epoch"), 1);
        assert_eq!(live(&mut conn), 0, "an unused sign-in link is spent");

        active_sessions::revoke_other_sessions_by_uuid(&mut conn, &user.uuid, &Uuid::now_v7())
            .expect("revoke others");
        assert_eq!(current(&mut conn, &user.uuid).expect("epoch"), 2);
    }
}
