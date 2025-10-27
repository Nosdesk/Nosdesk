use diesel::prelude::*;
use chrono::Utc;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{UserEmail, NewUserEmail};
use crate::schema::{user_emails, users};

/// Get all emails for a specific user
pub fn get_user_emails(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> Result<Vec<UserEmail>, diesel::result::Error> {
    user_emails::table
        .filter(user_emails::user_uuid.eq(user_uuid))
        .order(user_emails::is_primary.desc())
        .then_order_by(user_emails::created_at.asc())
        .load::<UserEmail>(conn)
}

/// Get all emails for a specific user by UUID (now redundant with get_user_emails, kept for compatibility)
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

/// Find a user by any of their email addresses
pub fn find_user_by_any_email(
    conn: &mut DbConnection,
    email: &str,
) -> Result<crate::models::User, diesel::result::Error> {
    use crate::schema::users;

    users::table
        .inner_join(user_emails::table.on(users::uuid.eq(user_emails::user_uuid)))
        .filter(user_emails::email.eq(email))
        .select(users::all_columns)
        .first::<crate::models::User>(conn)
}

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
            user_emails::updated_at.eq(Utc::now().naive_utc())
        ))
        .get_results(conn)
}

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
            .filter(user_emails::is_primary.eq(false)) // Never delete primary emails
    ).execute(conn)
}

/// Check if any of the provided emails belong to an existing user
pub fn find_user_by_any_of_emails(
    conn: &mut DbConnection,
    emails: &[String],
) -> Result<Option<crate::models::User>, diesel::result::Error> {
    use crate::schema::users;

    if emails.is_empty() {
        return Ok(None);
    }

    let result = users::table
        .inner_join(user_emails::table.on(users::uuid.eq(user_emails::user_uuid)))
        .filter(user_emails::email.eq_any(emails))
        .select(users::all_columns)
        .first::<crate::models::User>(conn)
        .optional()?;

    Ok(result)
} 