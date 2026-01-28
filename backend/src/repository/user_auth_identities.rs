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

// Get all auth identities for a user by UUID
pub fn get_user_identities(
    user_uuid: &Uuid,
    conn: &mut DbConnection,
) -> Result<Vec<UserAuthIdentity>, Error> {
    user_auth_identities::table
        .filter(user_auth_identities::user_uuid.eq(user_uuid))
        .load::<UserAuthIdentity>(conn)
}


// Get identities with provider info for display by UUID
pub fn get_user_identities_display(
    user_uuid: &Uuid,
    conn: &mut DbConnection,
) -> Result<Vec<UserAuthIdentityDisplay>, Error> {
    user_auth_identities::table
        .filter(user_auth_identities::user_uuid.eq(user_uuid))
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
) -> Result<Option<Uuid>, Error> {
    // Find the identity and return the user_uuid
    let result = user_auth_identities::table
        .filter(user_auth_identities::provider_type.eq(provider_type))
        .filter(user_auth_identities::external_id.eq(provider_user_id))
        .select(user_auth_identities::user_uuid)
        .first::<Uuid>(conn)
        .optional()?;

    Ok(result)
}

// Delete an auth identity by user UUID
pub fn delete_identity(
    identity_id: i32,
    user_uuid: &Uuid, // For security, ensure the identity belongs to this user
    conn: &mut DbConnection,
) -> Result<usize, Error> {
    diesel::delete(
        user_auth_identities::table
            .filter(user_auth_identities::id.eq(identity_id))
            .filter(user_auth_identities::user_uuid.eq(user_uuid))
    )
    .execute(conn)
}

/// Get user UUID by external ID (e.g., Microsoft Graph user ID)
/// Used for syncing group membership from external sources
#[allow(dead_code)]
pub fn get_user_uuid_by_external_id(
    external_id: &str,
    conn: &mut DbConnection,
) -> Result<Option<Uuid>, Error> {
    user_auth_identities::table
        .filter(user_auth_identities::external_id.eq(external_id))
        .select(user_auth_identities::user_uuid)
        .first::<Uuid>(conn)
        .optional()
}

/// Get user UUID by external ID for a specific provider type
#[allow(dead_code)]
pub fn get_user_uuid_by_external_id_and_provider(
    external_id: &str,
    provider_type: &str,
    conn: &mut DbConnection,
) -> Result<Option<Uuid>, Error> {
    user_auth_identities::table
        .filter(user_auth_identities::external_id.eq(external_id))
        .filter(user_auth_identities::provider_type.eq(provider_type))
        .select(user_auth_identities::user_uuid)
        .first::<Uuid>(conn)
        .optional()
}

/// Get multiple user UUIDs by their external IDs (batch lookup for efficiency)
pub fn get_user_uuids_by_external_ids(
    external_ids: &[&str],
    provider_type: &str,
    conn: &mut DbConnection,
) -> Result<Vec<(String, Uuid)>, Error> {
    user_auth_identities::table
        .filter(user_auth_identities::external_id.eq_any(external_ids))
        .filter(user_auth_identities::provider_type.eq(provider_type))
        .select((user_auth_identities::external_id, user_auth_identities::user_uuid))
        .load::<(String, Uuid)>(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewUserAuthIdentity, UserRole};
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    fn make_identity(user_uuid: Uuid, provider: &str, external_id: &str) -> NewUserAuthIdentity {
        NewUserAuthIdentity {
            user_uuid,
            provider_type: provider.to_string(),
            external_id: external_id.to_string(),
            email: None,
            metadata: None,
            password_hash: None,
        }
    }

    #[test]
    fn create_and_find_identity() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "iduser", UserRole::User);

        create_identity(make_identity(user.uuid, "github", "gh_123"), &mut conn).unwrap();

        let found = find_user_by_identity("github", "gh_123", &mut conn).unwrap();
        assert_eq!(found, Some(user.uuid));
    }

    #[test]
    fn delete_identity_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "delid", UserRole::User);

        let identity = create_identity(make_identity(user.uuid, "google", "g_456"), &mut conn).unwrap();
        let rows = delete_identity(identity.id, &user.uuid, &mut conn).unwrap();
        assert_eq!(rows, 1);

        let found = find_user_by_identity("google", "g_456", &mut conn).unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn get_user_identities_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "multiid", UserRole::User);

        create_identity(make_identity(user.uuid, "github", "gh_a"), &mut conn).unwrap();
        create_identity(make_identity(user.uuid, "google", "g_b"), &mut conn).unwrap();

        let identities = get_user_identities(&user.uuid, &mut conn).unwrap();
        assert!(identities.len() >= 2);
    }
}
