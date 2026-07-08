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
        .filter(user_emails::email.ilike(email)) // Case-insensitive match
        .select(users::all_columns)
        .first::<crate::models::User>(conn)
}

// sync-pending-wire: needs sync aggregate wiring
/// Add multiple emails for a user (used during Microsoft Graph sync)
pub fn add_multiple_emails(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    emails: Vec<(String, String, bool, String)>, // (email, type, verified, source)
) -> Result<Vec<UserEmail>, diesel::result::Error> {
    let new_emails: Vec<NewUserEmail> = emails
        .into_iter()
        .enumerate()
        .map(|(i, (email, email_type, verified, source))| NewUserEmail {
            user_uuid: *user_uuid,
            email,
            email_type,
            is_primary: i == 0, // First email is primary
            is_verified: verified,
            source: Some(source),
        })
        .collect();

    if new_emails.is_empty() {
        return Ok(Vec::new());
    }

    diesel::insert_into(user_emails::table)
        .values(&new_emails)
        .on_conflict(user_emails::email)
        .do_update()
        .set((
            user_emails::is_verified.eq(diesel::dsl::sql("EXCLUDED.is_verified")),
            user_emails::updated_at.eq(Utc::now().naive_utc()),
        ))
        .get_results(conn)
}

// sync-pending-wire: needs sync aggregate wiring
/// Remove emails for a user that are no longer present in the source system
pub fn cleanup_obsolete_emails(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    current_emails: &[String],
    _source: &str, // Source parameter kept for compatibility
) -> Result<usize, diesel::result::Error> {
    diesel::delete(
        user_emails::table
            .filter(user_emails::user_uuid.eq(user_uuid))
            .filter(user_emails::email.ne_all(current_emails))
            .filter(user_emails::is_primary.eq(false)), // Never delete primary emails
    )
    .execute(conn)
}

/// Check if any of the provided emails belong to an existing user (case-insensitive)
pub fn find_user_by_any_of_emails(
    conn: &mut DbConnection,
    emails: &[String],
) -> Result<Option<crate::models::User>, diesel::result::Error> {
    use crate::schema::users;

    if emails.is_empty() {
        return Ok(None);
    }

    // Normalize emails to lowercase for case-insensitive matching
    let normalized_emails: Vec<String> = emails.iter().map(|e| e.to_lowercase()).collect();

    let result = users::table
        .inner_join(user_emails::table.on(users::uuid.eq(user_emails::user_uuid)))
        .filter(user_emails::email.eq_any(&normalized_emails))
        .select(users::all_columns)
        .first::<crate::models::User>(conn)
        .optional()?;

    Ok(result)
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

// sync-audit-only: user_emails is a contact-detail table with no audit trigger and no sync aggregate; nothing subscribes to email add/update/remove
/// Mark a user's primary email as verified. Used on invitation accept,
/// where receiving the invite proves ownership of that address.
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
}
