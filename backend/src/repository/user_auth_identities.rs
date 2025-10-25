use diesel::prelude::*;
use diesel::result::Error;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{UserAuthIdentity, NewUserAuthIdentity, UserAuthIdentityDisplay};
use crate::schema::user_auth_identities;

// Create a new user auth identity
pub fn create_identity(
    new_identity: NewUserAuthIdentity,
    conn: &mut DbConnection,
) -> Result<UserAuthIdentity, Error> {
    diesel::insert_into(user_auth_identities::table)
        .values(new_identity)
        .get_result::<UserAuthIdentity>(conn)
}

// Get all auth identities for a user by ID
pub fn get_user_identities(
    user_id: i32,
    conn: &mut DbConnection,
) -> Result<Vec<UserAuthIdentity>, Error> {
    user_auth_identities::table
        .filter(user_auth_identities::user_id.eq(user_id))
        .load::<UserAuthIdentity>(conn)
}

// Get all auth identities for a user by UUID
pub fn get_user_identities_by_uuid(
    user_uuid: &Uuid,
    conn: &mut DbConnection,
) -> Result<Vec<UserAuthIdentity>, Error> {
    let user = crate::repository::users::get_user_by_uuid(user_uuid, conn)?;
    user_auth_identities::table
        .filter(user_auth_identities::user_id.eq(user.id))
        .load::<UserAuthIdentity>(conn)
}

// Get identities with provider info for display
pub fn get_user_identities_display(
    user_id: i32,
    conn: &mut DbConnection,
) -> Result<Vec<UserAuthIdentityDisplay>, Error> {
    user_auth_identities::table
        .filter(user_auth_identities::user_id.eq(user_id))
        .select((
            user_auth_identities::id,
            user_auth_identities::provider_type,
            user_auth_identities::provider_type, // Use provider_type as provider_name too
            user_auth_identities::email,
            user_auth_identities::created_at,
        ))
        .load::<(i32, String, String, Option<String>, chrono::NaiveDateTime)>(conn)
        .map(|results| {
            results
                .into_iter()
                .map(|(id, provider_type, provider_name, email, created_at)| {
                    UserAuthIdentityDisplay {
                        id,
                        provider_type,
                        provider_name,
                        email,
                        created_at,
                    }
                })
                .collect()
        })
}

// Get identities with provider info for display by UUID
pub fn get_user_identities_display_by_uuid(
    user_uuid: &Uuid,
    conn: &mut DbConnection,
) -> Result<Vec<UserAuthIdentityDisplay>, Error> {
    let user = crate::repository::users::get_user_by_uuid(user_uuid, conn)?;
    
    user_auth_identities::table
        .filter(user_auth_identities::user_id.eq(user.id))
        .select((
            user_auth_identities::id,
            user_auth_identities::provider_type,
            user_auth_identities::provider_type, // Use provider_type as provider_name too
            user_auth_identities::email,
            user_auth_identities::created_at,
        ))
        .load::<(i32, String, String, Option<String>, chrono::NaiveDateTime)>(conn)
        .map(|results| {
            results
                .into_iter()
                .map(|(id, provider_type, provider_name, email, created_at)| {
                    UserAuthIdentityDisplay {
                        id,
                        provider_type,
                        provider_name,
                        email,
                        created_at,
                    }
                })
                .collect()
        })
}

// Find user by their external identity (for auth)
pub fn find_user_by_identity(
    provider_type: &str,
    provider_user_id: &str,
    conn: &mut DbConnection,
) -> Result<Option<i32>, Error> {
    // Find the identity and return the user_id
    let result = user_auth_identities::table
        .filter(user_auth_identities::provider_type.eq(provider_type))
        .filter(user_auth_identities::external_id.eq(provider_user_id))
        .select(user_auth_identities::user_id)
        .first::<i32>(conn)
        .optional()?;
    
    Ok(result)
}

// Delete an auth identity
pub fn delete_identity(
    identity_id: i32,
    user_id: i32, // For security, ensure the identity belongs to this user
    conn: &mut DbConnection,
) -> Result<usize, Error> {
    diesel::delete(
        user_auth_identities::table
            .filter(user_auth_identities::id.eq(identity_id))
            .filter(user_auth_identities::user_id.eq(user_id))
    )
    .execute(conn)
}

// Delete an auth identity by user UUID
pub fn delete_identity_by_uuid(
    identity_id: i32,
    user_uuid: &Uuid, // For security, ensure the identity belongs to this user
    conn: &mut DbConnection,
) -> Result<usize, Error> {
    // Get the user ID from UUID
    let user = crate::repository::users::get_user_by_uuid(user_uuid, conn)?;
    
    diesel::delete(
        user_auth_identities::table
            .filter(user_auth_identities::id.eq(identity_id))
            .filter(user_auth_identities::user_id.eq(user.id))
    ).execute(conn)
}

 