use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewUserEmail, UserEmail, UserEmailUpdate};
use crate::schema::user_emails;

/// Get all emails for a specific user by UUID
pub fn get_user_emails_by_uuid(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> Result<Vec<UserEmail>, diesel::result::Error> {
    user_emails::table
        .filter(user_emails::user_uuid.eq(user_uuid))
        .order(user_emails::is_primary.desc())
        .then_order_by(user_emails::created_at.asc())
        .load::<UserEmail>(conn)
}

/// Find a user by any of their email addresses (case-insensitive)
pub fn find_user_by_any_email(
    conn: &mut DbConnection,
    email: &str,
) -> Result<crate::models::User, diesel::result::Error> {
    use crate::schema::users;

    users::table
        .inner_join(user_emails::table.on(users::uuid.eq(user_emails::user_uuid)))
        .filter(user_emails::email.ilike(super::escape_like(email))) // Case-insensitive match
        .select(users::all_columns)
        .first::<crate::models::User>(conn)
}

diesel::define_sql_function! {
    fn lower(x: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

/// Every row holding `email`, compared case-insensitively: addresses are
/// unique by their exact text, but every lookup matches them
/// case-insensitively, so case variants held by two accounts both answer.
/// Served by the `lower(email)` index.
pub fn holders_of(conn: &mut DbConnection, email: &str) -> QueryResult<Vec<UserEmail>> {
    user_emails::table
        .filter(lower(user_emails::email).eq(email.trim().to_lowercase()))
        .order(user_emails::id.asc())
        .load(conn)
}

// sync-pending-wire: needs sync aggregate wiring
/// Add multiple emails for a user (used during Microsoft Graph sync), stored
/// lowercase. An address the user already holds, in any case, has its
/// verified flag refreshed; one another account holds, in any case, is left
/// as it is and not returned. A new address becomes the primary one only for
/// a user who has none.
pub fn add_multiple_emails(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    emails: Vec<(String, String, bool, String)>, // (email, type, verified, source)
) -> Result<Vec<UserEmail>, diesel::result::Error> {
    let mut has_primary = user_emails::table
        .filter(user_emails::user_uuid.eq(user_uuid))
        .filter(user_emails::is_primary.eq(true))
        .count()
        .get_result::<i64>(conn)?
        > 0;
    let mut stored = Vec::new();
    for (email, email_type, verified, source) in emails {
        let email = email.trim().to_lowercase();
        if email.is_empty() {
            continue;
        }
        let held = holders_of(conn, &email)?;
        match held.first() {
            // Another account holds it, in some case.
            Some(_) if held.iter().any(|r| r.user_uuid != *user_uuid) => {}
            Some(row) => stored.push(
                diesel::update(user_emails::table.find(row.id))
                    .set((
                        user_emails::is_verified.eq(verified),
                        user_emails::updated_at.eq(Utc::now().naive_utc()),
                    ))
                    .get_result::<UserEmail>(conn)?,
            ),
            None => {
                let row = diesel::insert_into(user_emails::table)
                    .values(&NewUserEmail {
                        user_uuid: *user_uuid,
                        email,
                        email_type,
                        is_primary: !has_primary,
                        is_verified: verified,
                        source: Some(source),
                    })
                    .on_conflict(user_emails::email)
                    .do_nothing()
                    .get_result::<UserEmail>(conn)
                    .optional()?;
                if let Some(row) = row {
                    has_primary |= row.is_primary;
                    stored.push(row);
                }
            }
        }
    }
    Ok(stored)
}

/// Every holder of any of `emails`, compared case-insensitively, with whether
/// that row is verified. For a directory sync deciding whether a person it
/// lists is an account it may link, a stranger's, or new.
pub fn address_holders(
    conn: &mut DbConnection,
    emails: &[String],
) -> QueryResult<Vec<(Uuid, bool)>> {
    let mut out = Vec::new();
    for email in emails {
        out.extend(
            holders_of(conn, email)?
                .into_iter()
                .map(|row| (row.user_uuid, row.is_verified)),
        );
    }
    Ok(out)
}

// sync-pending-wire: needs sync aggregate wiring
/// Remove the user's addresses that `source` added and no longer lists
/// (compared case-insensitively). An address from anywhere else (added by
/// hand, by another provider) stays.
pub fn cleanup_obsolete_emails(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    current_emails: &[String],
    source: &str,
) -> Result<usize, diesel::result::Error> {
    let current: Vec<String> = current_emails
        .iter()
        .map(|e| e.trim().to_lowercase())
        .collect();
    diesel::delete(
        user_emails::table
            .filter(user_emails::user_uuid.eq(user_uuid))
            .filter(user_emails::source.eq(source))
            .filter(lower(user_emails::email).ne_all(current))
            .filter(user_emails::is_primary.eq(false)), // Never delete primary emails
    )
    .execute(conn)
}

// sync-pending-wire: needs sync aggregate wiring
/// Remove every non-primary address of the user that `keep` doesn't list,
/// whatever added it. For the control plane, which owns a hosted account's
/// whole verified set; a directory sync cleans up only its own rows
/// ([`cleanup_obsolete_emails`]).
pub fn remove_unlisted_addresses(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    keep: &[String],
) -> Result<usize, diesel::result::Error> {
    diesel::delete(
        user_emails::table
            .filter(user_emails::user_uuid.eq(user_uuid))
            .filter(user_emails::email.ne_all(keep))
            .filter(user_emails::is_primary.eq(false)),
    )
    .execute(conn)
}

/// Look up a single email row by its numeric id.
pub fn get_email_by_id(
    conn: &mut DbConnection,
    email_id: i32,
) -> Result<UserEmail, diesel::result::Error> {
    user_emails::table.find(email_id).first::<UserEmail>(conn)
}

// sync-audit-only: user_emails is a contact-detail table with no audit trigger and no sync aggregate; nothing subscribes to email add/update/remove
/// Insert one email row for a user and return the created record.
pub fn add_email(
    conn: &mut DbConnection,
    new_email: &NewUserEmail,
) -> Result<UserEmail, diesel::result::Error> {
    diesel::insert_into(user_emails::table)
        .values(new_email)
        .get_result::<UserEmail>(conn)
}

// sync-audit-only: user_emails is a contact-detail table with no audit trigger and no sync aggregate; nothing subscribes to email add/update/remove
/// Clear the `is_primary` flag on every email a user owns. Used before
/// promoting a different address so at most one stays primary.
pub fn clear_primary(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::update(user_emails::table.filter(user_emails::user_uuid.eq(user_uuid)))
        .set(user_emails::is_primary.eq(false))
        .execute(conn)
}

// sync-audit-only: user_emails is a contact-detail table with no audit trigger and no sync aggregate; nothing subscribes to email add/update/remove
/// Apply a partial update (primary / verified flags) to one email row.
pub fn update_email(
    conn: &mut DbConnection,
    email_id: i32,
    changes: &UserEmailUpdate,
) -> Result<UserEmail, diesel::result::Error> {
    diesel::update(user_emails::table.find(email_id))
        .set(changes)
        .get_result::<UserEmail>(conn)
}

/// Make `email_id` the user's primary address: clear the flag on their other
/// addresses, set it on this one, and send `user.updated` to every workspace
/// holding the person, whose pooled row carries the primary address. One
/// transaction, so a failure can't leave the user with none.
pub fn set_primary(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    email_id: i32,
    changes: &UserEmailUpdate,
) -> Result<UserEmail, diesel::result::Error> {
    conn.transaction(|conn| {
        clear_primary(conn, user_uuid)?;
        let updated = diesel::update(user_emails::table.find(email_id))
            .set(changes)
            .get_result::<UserEmail>(conn)?;
        // emit::record fires inside the user writer's emit_user_event.
        crate::repository::users::emit_user_updated_everywhere(conn, *user_uuid)?;
        Ok(updated)
    })
}

// sync-audit-only: user_emails is a contact-detail table with no audit trigger and no sync aggregate; nothing subscribes to email add/update/remove
/// Mark a user's primary email as verified, for a source that vouches for it
/// without its mailbox proving anything: the workspace's own sign-in, a
/// directory. Sets `is_verified` (the inbound-mail sender guard) and leaves
/// `proven_at` alone; see [`mark_primary_proven`].
pub fn mark_primary_verified(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::update(
        user_emails::table
            .filter(user_emails::user_uuid.eq(user_uuid))
            .filter(user_emails::is_primary.eq(true)),
    )
    .set(user_emails::is_verified.eq(true))
    .execute(conn)
}

/// The database's clock, for `proven_at`.
fn db_now(
) -> diesel::expression::SqlLiteral<diesel::sql_types::Nullable<diesel::sql_types::Timestamptz>> {
    diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Timestamptz>>("now()")
}

// sync-audit-only: user_emails is a contact-detail table with no audit trigger and no sync aggregate; nothing subscribes to email add/update/remove
/// Mark a user's primary email verified and proven now: its mailbox answered.
/// Only for a step a mail scanner can't take: a single-use sign-in link
/// confirmed with a POST, a sign-in code, or an invitation or request
/// confirmation (a POST). A multi-use link in an email ("View request",
/// "Review request") proves nothing, and neither does a directory or a
/// workspace's own sign-in; those don't call this. `proven_at` decides whether
/// platform-sender mail to the address carries the workspace's own branding
/// (`utils::email_branding::instance_mail_for`).
pub fn mark_primary_proven(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::update(
        user_emails::table
            .filter(user_emails::user_uuid.eq(user_uuid))
            .filter(user_emails::is_primary.eq(true)),
    )
    .set((
        user_emails::is_verified.eq(true),
        user_emails::proven_at.eq(db_now()),
    ))
    .execute(conn)
}

// sync-audit-only: user_emails is a contact-detail table with no audit trigger and no sync aggregate; nothing subscribes to email add/update/remove
/// Mark verified the user's row for `address`, compared case-insensitively:
/// a sign-in link sent there was followed. A row the user no longer holds is
/// left alone (0 rows).
pub fn mark_address_verified(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    address: &str,
) -> Result<usize, diesel::result::Error> {
    diesel::update(
        user_emails::table
            .filter(user_emails::user_uuid.eq(user_uuid))
            .filter(lower(user_emails::email).eq(address.trim().to_lowercase())),
    )
    .set(user_emails::is_verified.eq(true))
    .execute(conn)
}

// sync-audit-only: user_emails is a contact-detail table with no audit trigger and no sync aggregate; nothing subscribes to email add/update/remove
/// [`mark_address_verified`] and proven now (`proven_at`, the database's
/// clock): the mailbox at `address` answered a single-use sign-in link's POST
/// or code, or a confirmation. See [`mark_primary_proven`].
pub fn mark_address_proven(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    address: &str,
) -> Result<usize, diesel::result::Error> {
    diesel::update(
        user_emails::table
            .filter(user_emails::user_uuid.eq(user_uuid))
            .filter(lower(user_emails::email).eq(address.trim().to_lowercase())),
    )
    .set((
        user_emails::is_verified.eq(true),
        user_emails::proven_at.eq(db_now()),
    ))
    .execute(conn)
}

// sync-audit-only: user_emails is a contact-detail table with no audit trigger and no sync aggregate; nothing subscribes to email add/update/remove
/// Mark one address verified, but only if the row still matches the claim the
/// token was minted against.
///
/// Both predicates matter. `user_uuid` stops a token verifying a row on another
/// account, and `email` stops it verifying a row that has since been pointed at
/// a different address: the proof was of the address in the token, not of
/// whatever that row holds when the link is finally clicked.
///
/// The address is required rather than optional so there is no call shape in
/// which the second check is skipped.
///
/// Returns the number of rows updated, so 0 means "no longer matches" rather
/// than an error.
pub fn mark_verified_if_matches(
    conn: &mut DbConnection,
    email_id: i32,
    user_uuid: Uuid,
    address: &str,
) -> Result<usize, diesel::result::Error> {
    diesel::update(
        user_emails::table
            .find(email_id)
            .filter(user_emails::user_uuid.eq(user_uuid))
            .filter(user_emails::email.eq(address)),
    )
    .set((
        user_emails::is_verified.eq(true),
        user_emails::proven_at.eq(db_now()),
    ))
    .execute(conn)
}

// sync-audit-only: user_emails is a contact-detail table with no audit trigger and no sync aggregate; nothing subscribes to email add/update/remove
/// Remove one email row by id.
pub fn delete_email(
    conn: &mut DbConnection,
    email_id: i32,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(user_emails::table.find(email_id)).execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn get_user_emails_by_uuid_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "emailuser", "user");

        TestFixtures::create_user_email(&mut conn, user.uuid, "one@example.com", true);
        TestFixtures::create_user_email(&mut conn, user.uuid, "two@example.com", false);

        let emails = get_user_emails_by_uuid(&mut conn, &user.uuid).unwrap();
        assert_eq!(emails.len(), 2);
        let addrs: Vec<&str> = emails.iter().map(|e| e.email.as_str()).collect();
        assert!(addrs.contains(&"one@example.com"));
        assert!(addrs.contains(&"two@example.com"));
    }

    #[test]
    fn find_user_by_any_email_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "findme", "user");
        TestFixtures::create_user_email(&mut conn, user.uuid, "findme@example.com", true);

        let found = find_user_by_any_email(&mut conn, "findme@example.com").unwrap();
        assert_eq!(found.uuid, user.uuid);
    }

    #[test]
    fn find_user_by_any_email_case_insensitive() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "caseuser", "user");
        TestFixtures::create_user_email(&mut conn, user.uuid, "Test@Example.com", true);

        let found = find_user_by_any_email(&mut conn, "test@example.com").unwrap();
        assert_eq!(found.uuid, user.uuid);
    }

    /// The update changeset must have no way to express "verified".
    ///
    /// `PUT /users/{uuid}/emails/{email_id}` is authorised by "you are this
    /// user", and it used to read `is_verified` from the request body, so any
    /// user could assert their own address verified. That flag is the inbound
    /// mail impersonation guard (`user_helpers::find_verified_user_by_email`),
    /// so asserting it made the channel pipeline attribute mail from the
    /// address to whoever claimed it.
    ///
    /// Verification is only ever written where something proves ownership:
    /// account creation from the provider's `email_verified` claim, and
    /// [`mark_primary_verified`] on invitation accept. Neither is a request
    /// body.
    #[test]
    fn an_update_cannot_assert_verification() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "selfverify", "user");
        let email =
            TestFixtures::create_user_email(&mut conn, user.uuid, "claimed@example.com", true);
        diesel::update(user_emails::table.find(email.id))
            .set(user_emails::is_verified.eq(false))
            .execute(&mut conn)
            .expect("start unverified");

        // Everything the handler is able to build from a request body.
        let changes = UserEmailUpdate {
            is_primary: Some(true),
            updated_at: Some(chrono::Utc::now().naive_utc()),
        };
        let updated = update_email(&mut conn, email.id, &changes).expect("update");

        assert!(
            !updated.is_verified,
            "an update built from a request body must not be able to verify an \
             address; if this fails, `is_verified` is back on UserEmailUpdate"
        );
        assert!(updated.is_primary, "the fields it may set still apply");
    }

    /// The two predicates on the update are the whole guard, so each gets a
    /// case that fails if it is dropped.
    #[test]
    fn verification_applies_only_to_the_row_the_token_named() {
        let mut conn = setup_test_connection();
        let owner = TestFixtures::create_user(&mut conn, "owner", "user");
        let other = TestFixtures::create_user(&mut conn, "other", "user");
        let email =
            TestFixtures::create_user_email(&mut conn, owner.uuid, "claimed@example.com", true);
        diesel::update(user_emails::table.find(email.id))
            .set(user_emails::is_verified.eq(false))
            .execute(&mut conn)
            .expect("start unverified");

        // Right row, right address: verified.
        assert_eq!(
            mark_verified_if_matches(&mut conn, email.id, owner.uuid, "claimed@example.com")
                .expect("update"),
            1
        );

        diesel::update(user_emails::table.find(email.id))
            .set(user_emails::is_verified.eq(false))
            .execute(&mut conn)
            .expect("reset");

        // Another account's token must not verify this row.
        assert_eq!(
            mark_verified_if_matches(&mut conn, email.id, other.uuid, "claimed@example.com")
                .expect("update"),
            0,
            "a token minted for one account must not verify another account's address"
        );

        // A token minted against a different address must not verify whatever
        // the row holds now.
        assert_eq!(
            mark_verified_if_matches(&mut conn, email.id, owner.uuid, "stale@example.com")
                .expect("update"),
            0,
            "the proof was of the address in the token, not of the row"
        );

        let after = get_user_emails_by_uuid(&mut conn, &owner.uuid)
            .expect("reload")
            .into_iter()
            .find(|e| e.id == email.id)
            .expect("row present");
        assert!(
            !after.is_verified,
            "neither rejected call may have verified the address"
        );
    }
}
