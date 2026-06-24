use diesel::prelude::*;
use diesel::result::Error;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewUserAuthIdentity, UserAuthIdentity, UserAuthIdentityDisplay};
use crate::schema::user_auth_identities;

// Create a new user auth identity
// sync-pending-wire: needs sync aggregate wiring
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

/// Find a user by their GLOBAL login identity (local/microsoft/oidc), keyed on
/// (provider_type, external_id) across the instance. Excludes workspace-scoped
/// directory identities (ldap/scim) via the `workspace_id IS NULL` filter.
pub fn find_user_by_identity(
    provider_type: &str,
    provider_user_id: &str,
    conn: &mut DbConnection,
) -> Result<Option<Uuid>, Error> {
    let result = user_auth_identities::table
        .filter(user_auth_identities::provider_type.eq(provider_type))
        .filter(user_auth_identities::external_id.eq(provider_user_id))
        .filter(user_auth_identities::workspace_id.is_null())
        .select(user_auth_identities::user_uuid)
        .first::<Uuid>(conn)
        .optional()?;

    Ok(result)
}

/// Find a user by a WORKSPACE-SCOPED directory identity (ldap/scim), keyed
/// within the workspace. The same external_id can resolve to different users in
/// different workspaces; this is the lookup the directory sync + auth paths use.
pub fn find_user_by_scoped_identity(
    workspace_id: i32,
    provider_type: &str,
    external_id: &str,
    conn: &mut DbConnection,
) -> Result<Option<Uuid>, Error> {
    user_auth_identities::table
        .filter(user_auth_identities::workspace_id.eq(workspace_id))
        .filter(user_auth_identities::provider_type.eq(provider_type))
        .filter(user_auth_identities::external_id.eq(external_id))
        .select(user_auth_identities::user_uuid)
        .first::<Uuid>(conn)
        .optional()
}

// Delete an auth identity by user UUID
// sync-pending-wire: needs sync aggregate wiring
pub fn delete_identity(
    identity_id: i32,
    user_uuid: &Uuid, // For security, ensure the identity belongs to this user
    conn: &mut DbConnection,
) -> Result<usize, Error> {
    diesel::delete(
        user_auth_identities::table
            .filter(user_auth_identities::id.eq(identity_id))
            .filter(user_auth_identities::user_uuid.eq(user_uuid)),
    )
    .execute(conn)
}

// sync-pending-wire: needs sync aggregate wiring
/// Replace the password hash on a user's `local` auth identity.
/// Used by the CLI admin password-reset path. Returns the number of
/// rows updated, which the caller can use to confirm the user
/// actually had a local identity (a Microsoft/OIDC-only account
/// would hit zero rows).
pub fn update_local_password_hash(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    new_hash: &str,
) -> Result<usize, Error> {
    diesel::update(
        user_auth_identities::table
            .filter(user_auth_identities::user_uuid.eq(user_uuid))
            .filter(user_auth_identities::provider_type.eq("local")),
    )
    .set(user_auth_identities::password_hash.eq(new_hash))
    .execute(conn)
}

/// Get the local-auth password hash for a user, if they have a local
/// identity with a password set.
///
/// Returns the collapsed `Result<String, String>` shape the auth and
/// passkey handlers consume: a DB failure and a missing local password
/// both surface as `Err`, and callers only ever distinguish "got a
/// hash" from "didn't". Previously duplicated verbatim in
/// `handlers/auth.rs` and `handlers/passkeys.rs`.
pub fn get_local_password_hash(
    user_uuid: &Uuid,
    conn: &mut DbConnection,
) -> Result<String, String> {
    let password_hash: Option<String> = user_auth_identities::table
        .filter(user_auth_identities::user_uuid.eq(user_uuid))
        .filter(user_auth_identities::provider_type.eq("local"))
        .select(user_auth_identities::password_hash)
        .first::<Option<String>>(conn)
        .optional()
        .map_err(|e| format!("Database error: {e}"))?
        .flatten();

    password_hash.ok_or_else(|| "No local password found for this user".to_string())
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
        .filter(user_auth_identities::workspace_id.is_null())
        .select((
            user_auth_identities::external_id,
            user_auth_identities::user_uuid,
        ))
        .load::<(String, Uuid)>(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewUserAuthIdentity;
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
        let user = TestFixtures::create_user(&mut conn, "iduser", "user");

        create_identity(make_identity(user.uuid, "github", "gh_123"), &mut conn).unwrap();

        let found = find_user_by_identity("github", "gh_123", &mut conn).unwrap();
        assert_eq!(found, Some(user.uuid));
    }

    #[test]
    fn delete_identity_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "delid", "user");

        let identity =
            create_identity(make_identity(user.uuid, "google", "g_456"), &mut conn).unwrap();
        let rows = delete_identity(identity.id, &user.uuid, &mut conn).unwrap();
        assert_eq!(rows, 1);

        let found = find_user_by_identity("google", "g_456", &mut conn).unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn get_user_identities_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "multiid", "user");

        create_identity(make_identity(user.uuid, "github", "gh_a"), &mut conn).unwrap();
        create_identity(make_identity(user.uuid, "google", "g_b"), &mut conn).unwrap();

        let identities = get_user_identities(&user.uuid, &mut conn).unwrap();
        assert!(identities.len() >= 2);
    }

    #[test]
    fn global_and_scoped_identities_coexist() {
        let mut conn = setup_test_connection();
        let global_user = TestFixtures::create_user(&mut conn, "globalid", "user");
        let scoped_user = TestFixtures::create_user(&mut conn, "scopedid", "user");

        // Same (provider_type, external_id): one global (workspace_id NULL), one
        // directory-scoped (workspace 1). The two partial uniques let them coexist.
        create_identity(
            make_identity(global_user.uuid, "ldap", "uid-shared"),
            &mut conn,
        )
        .unwrap();
        diesel::insert_into(user_auth_identities::table)
            .values((
                user_auth_identities::user_uuid.eq(scoped_user.uuid),
                user_auth_identities::provider_type.eq("ldap"),
                user_auth_identities::external_id.eq("uid-shared"),
                user_auth_identities::workspace_id.eq(Some(1)),
            ))
            .execute(&mut conn)
            .expect("a global + a scoped identity with the same external_id must coexist");

        // Each lookup resolves to its own side without crossing the boundary.
        assert_eq!(
            find_user_by_identity("ldap", "uid-shared", &mut conn).unwrap(),
            Some(global_user.uuid)
        );
        assert_eq!(
            find_user_by_scoped_identity(1, "ldap", "uid-shared", &mut conn).unwrap(),
            Some(scoped_user.uuid)
        );
    }

    #[test]
    fn global_identity_uniqueness_still_enforced() {
        let mut conn = setup_test_connection();
        let u1 = TestFixtures::create_user(&mut conn, "dup1", "user");
        let u2 = TestFixtures::create_user(&mut conn, "dup2", "user");

        create_identity(make_identity(u1.uuid, "oidc", "sub-x"), &mut conn).unwrap();
        let dup = create_identity(make_identity(u2.uuid, "oidc", "sub-x"), &mut conn);
        assert!(
            dup.is_err(),
            "a duplicate global identity must violate the partial unique"
        );
    }
}
