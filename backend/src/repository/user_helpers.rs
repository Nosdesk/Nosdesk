use diesel::prelude::*;
use uuid::Uuid;
use crate::db::DbConnection;
use crate::models::{User, UserEmail};

/// Get a user's primary email address
/// This is now the canonical way to get a user's email since users table no longer has email field
pub fn get_primary_email(user_uuid: &Uuid, conn: &mut DbConnection) -> Option<String> {
    use crate::schema::user_emails;

    user_emails::table
        .filter(user_emails::user_uuid.eq(user_uuid))
        .filter(user_emails::is_primary.eq(true))
        .select(user_emails::email)
        .first::<String>(conn)
        .ok()
}

/// Get user by email address (looks up in user_emails table)
/// SECURITY: Only matches PRIMARY emails - secondary emails cannot be used for login
/// This follows industry best practices (Google, Microsoft, GitHub, etc.)
/// NOTE: Email comparison is case-insensitive per RFC 5321
pub fn get_user_by_email(email: &str, conn: &mut DbConnection) -> Result<User, diesel::result::Error> {
    use crate::schema::{users, user_emails};

    users::table
        .inner_join(user_emails::table.on(users::uuid.eq(user_emails::user_uuid)))
        .filter(user_emails::email.ilike(email)) // Case-insensitive match
        .filter(user_emails::is_primary.eq(true)) // Only allow login with primary email
        .select(users::all_columns)
        .first::<User>(conn)
}

/// Create a user with their primary email atomically
/// This ensures consistency between users and user_emails tables
pub fn create_user_with_email(
    new_user: crate::models::NewUser,
    email: String,
    email_verified: bool,
    email_source: Option<String>,
    conn: &mut DbConnection,
) -> Result<(User, UserEmail), diesel::result::Error> {
    conn.transaction::<_, diesel::result::Error, _>(|conn| {
        // Create user first
        let user: User = diesel::insert_into(crate::schema::users::table)
            .values(&new_user)
            .get_result(conn)?;

        // Then create primary email
        let new_email = crate::models::NewUserEmail {
            user_uuid: user.uuid,
            email: email.clone(),
            email_type: "personal".to_string(),
            is_primary: true,
            is_verified: email_verified,
            source: email_source,
        };

        let user_email = diesel::insert_into(crate::schema::user_emails::table)
            .values(&new_email)
            .get_result(conn)?;

        Ok((user, user_email))
    })
}

/// Helper to get user with their primary email for responses
pub fn get_user_with_primary_email(
    user: crate::models::User,
    conn: &mut DbConnection,
) -> crate::models::UserResponse {
    let primary_email = get_primary_email(&user.uuid, conn);

    crate::models::UserResponse {
        uuid: user.uuid,
        name: user.name,
        email: primary_email, // Fetched from user_emails table
        role: user.role,
        pronouns: user.pronouns,
        avatar_url: user.avatar_url,
        banner_url: user.banner_url,
        avatar_thumb: user.avatar_thumb,
        theme: user.theme,
        microsoft_uuid: user.microsoft_uuid,
        created_at: user.created_at,
        updated_at: user.updated_at,
    }
}

/// Batch get primary emails for multiple users efficiently
/// Returns a HashMap of user_uuid -> email
pub fn get_primary_emails_batch(
    user_uuids: &[Uuid],
    conn: &mut DbConnection,
) -> std::collections::HashMap<Uuid, String> {
    use crate::schema::user_emails;

    let emails: Vec<(Uuid, String)> = user_emails::table
        .filter(user_emails::user_uuid.eq_any(user_uuids))
        .filter(user_emails::is_primary.eq(true))
        .select((user_emails::user_uuid, user_emails::email))
        .load::<(Uuid, String)>(conn)
        .unwrap_or_default();

    emails.into_iter().collect()
}

/// Helper to convert multiple users to UserResponses with their emails
pub fn get_users_with_primary_emails(
    users: Vec<crate::models::User>,
    conn: &mut DbConnection,
) -> Vec<crate::models::UserResponse> {
    // Collect all user UUIDs
    let user_uuids: Vec<Uuid> = users.iter().map(|u| u.uuid).collect();

    // Batch fetch all primary emails
    let email_map = get_primary_emails_batch(&user_uuids, conn);

    // Convert users to UserResponses with their emails
    users.into_iter().map(|user| {
        let email = email_map.get(&user.uuid).cloned();
        crate::models::UserResponse {
            uuid: user.uuid,
            name: user.name,
            email,
            role: user.role,
            pronouns: user.pronouns,
            avatar_url: user.avatar_url,
            banner_url: user.banner_url,
            avatar_thumb: user.avatar_thumb,
            theme: user.theme,
            microsoft_uuid: user.microsoft_uuid,
            created_at: user.created_at,
            updated_at: user.updated_at,
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use crate::models::UserRole;

    #[test]
    fn get_primary_email_returns_primary() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "emailuser", UserRole::User);
        TestFixtures::create_user_email(&mut conn, user.uuid, "primary@test.com", true);
        TestFixtures::create_user_email(&mut conn, user.uuid, "secondary@test.com", false);

        let email = get_primary_email(&user.uuid, &mut conn);
        assert_eq!(email, Some("primary@test.com".to_string()));
    }

    #[test]
    fn get_primary_email_returns_none_when_missing() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "noemail", UserRole::User);

        assert_eq!(get_primary_email(&user.uuid, &mut conn), None);
    }

    #[test]
    fn get_user_by_email_case_insensitive() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "ciuser", UserRole::User);
        TestFixtures::create_user_email(&mut conn, user.uuid, "alice@example.com", true);

        let found = get_user_by_email("ALICE@EXAMPLE.COM", &mut conn).unwrap();
        assert_eq!(found.uuid, user.uuid);
    }

    #[test]
    fn get_user_by_email_only_matches_primary() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "prionly", UserRole::User);
        TestFixtures::create_user_email(&mut conn, user.uuid, "real@test.com", true);
        TestFixtures::create_user_email(&mut conn, user.uuid, "secondary@test.com", false);

        assert!(get_user_by_email("secondary@test.com", &mut conn).is_err());
        assert!(get_user_by_email("real@test.com", &mut conn).is_ok());
    }

    #[test]
    fn create_user_with_email_atomic() {
        let mut conn = setup_test_connection();
        let new_user = crate::models::NewUser {
            uuid: Uuid::new_v4(),
            name: "Atomic".into(),
            role: UserRole::User,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            theme: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
            passkey_credentials: None,
        };

        let (user, email_record) = create_user_with_email(
            new_user, "atomic@test.com".into(), true, None, &mut conn,
        ).unwrap();

        assert_eq!(user.name, "Atomic");
        assert_eq!(email_record.email, "atomic@test.com");
        assert!(email_record.is_primary);
        assert!(email_record.is_verified);
    }

    #[test]
    fn batch_primary_emails() {
        let mut conn = setup_test_connection();
        let u1 = TestFixtures::create_user(&mut conn, "batch1", UserRole::User);
        let u2 = TestFixtures::create_user(&mut conn, "batch2", UserRole::User);
        TestFixtures::create_user_email(&mut conn, u1.uuid, "b1@test.com", true);
        TestFixtures::create_user_email(&mut conn, u2.uuid, "b2@test.com", true);

        let map = get_primary_emails_batch(&[u1.uuid, u2.uuid], &mut conn);
        assert_eq!(map.get(&u1.uuid), Some(&"b1@test.com".to_string()));
        assert_eq!(map.get(&u2.uuid), Some(&"b2@test.com".to_string()));
    }
}
