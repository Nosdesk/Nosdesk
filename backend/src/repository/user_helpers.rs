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
pub fn get_user_by_email(email: &str, conn: &mut DbConnection) -> Result<User, diesel::result::Error> {
    use crate::schema::{users, user_emails};

    users::table
        .inner_join(user_emails::table.on(users::uuid.eq(user_emails::user_uuid)))
        .filter(user_emails::email.eq(email))
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
