//! CRUD for reusable reply templates. Shared across the team; any
//! authenticated user can read, admins can mutate (enforcement is
//! at the handler level).

use diesel::prelude::*;
use diesel::QueryResult;

use crate::db::DbConnection;
use crate::models::{CannedResponse, CannedResponseUpdate, NewCannedResponse};

pub fn list(conn: &mut DbConnection) -> QueryResult<Vec<CannedResponse>> {
    use crate::schema::canned_responses::dsl::*;
    canned_responses.order(title.asc()).load(conn)
}

pub fn find(conn: &mut DbConnection, row_id: i32) -> QueryResult<CannedResponse> {
    use crate::schema::canned_responses::dsl::*;
    canned_responses.find(row_id).first(conn)
}

// sync-pending-wire: needs sync aggregate wiring
pub fn create(conn: &mut DbConnection, new: NewCannedResponse) -> QueryResult<CannedResponse> {
    use crate::schema::canned_responses::dsl::*;
    diesel::insert_into(canned_responses)
        .values(&new)
        .get_result(conn)
}

// sync-pending-wire: needs sync aggregate wiring
pub fn update(
    conn: &mut DbConnection,
    row_id: i32,
    mut change: CannedResponseUpdate,
) -> QueryResult<CannedResponse> {
    use crate::schema::canned_responses::dsl::*;
    change.updated_at = Some(chrono::Utc::now().naive_utc());
    diesel::update(canned_responses.find(row_id))
        .set(&change)
        .get_result(conn)
}

// sync-pending-wire: needs sync aggregate wiring
pub fn delete(conn: &mut DbConnection, row_id: i32) -> QueryResult<usize> {
    use crate::schema::canned_responses::dsl::*;
    diesel::delete(canned_responses.find(row_id)).execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn crud_roundtrip() {
        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "admin", UserRole::Admin);
        let created = create(
            &mut conn,
            NewCannedResponse {
                title: "Password reset".into(),
                body: "Hi {{customer_name}}, please follow this link...".into(),
                created_by: Some(admin.uuid),
            },
        )
        .unwrap();
        assert!(created.id > 0);
        assert_eq!(created.title, "Password reset");

        let updated = update(
            &mut conn,
            created.id,
            CannedResponseUpdate {
                title: Some("Password reset link".into()),
                body: None,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(updated.title, "Password reset link");
        assert_eq!(updated.body, created.body, "body stays unchanged");

        let all = list(&mut conn).unwrap();
        assert!(all.iter().any(|r| r.id == created.id));

        let removed = delete(&mut conn, created.id).unwrap();
        assert_eq!(removed, 1);
        assert!(find(&mut conn, created.id).is_err());
    }
}
