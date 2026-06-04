//! CRUD for reusable reply templates. Shared across the team; any
//! authenticated user can read, admins can mutate (enforcement is
//! at the handler level).

use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::BigInt;
use diesel::QueryResult;
use std::collections::HashMap;

use crate::db::DbConnection;
use crate::models::{
    CannedResponse, CannedResponseListItem, CannedResponseUpdate, NewCannedResponse,
    NewCannedResponseInsertion,
};

pub fn list(conn: &mut DbConnection) -> QueryResult<Vec<CannedResponse>> {
    use crate::schema::canned_responses::dsl::*;
    canned_responses.order(title.asc()).load(conn)
}

/// List every canned response paired with its 30-day insertion
/// count. The counter comes from a separate aggregate query rather
/// than a JOIN-with-COUNT-FILTER so the SQL stays inside Diesel's
/// query builder and the canned-responses table read stays a clean
/// `*` SELECT. Two trips is negligible at the expected catalog size
/// (canned responses are O(100) per workspace at most).
pub fn list_with_insert_counts(
    conn: &mut DbConnection,
) -> QueryResult<Vec<CannedResponseListItem>> {
    use crate::schema::canned_response_insertions::dsl as ins;
    use crate::schema::canned_responses::dsl::*;

    let rows: Vec<CannedResponse> = canned_responses.order(title.asc()).load(conn)?;

    // Aggregate insertions for the last 30 days into a map keyed by
    // canned_response_id. Templates with no recent insertions don't
    // appear in the map; we fall back to 0 when stitching.
    let counts: Vec<(i32, i64)> = ins::canned_response_insertions
        .filter(ins::inserted_at.gt(sql("NOW() - INTERVAL '30 days'")))
        .group_by(ins::canned_response_id)
        .select((ins::canned_response_id, sql::<BigInt>("COUNT(*)")))
        .load(conn)?;
    let count_by_id: HashMap<i32, i64> = counts.into_iter().collect();

    Ok(rows
        .into_iter()
        .map(|row| {
            let n = count_by_id.get(&row.id).copied().unwrap_or(0);
            CannedResponseListItem::from_parts(row, n)
        })
        .collect())
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

/// Append one insertion record. Fire-and-forget from the picker;
/// callers don't fail the user-facing insert on a logging error.
/// Workspace-local usage counter, never propagated to clients (no
/// list endpoint, no entity surface, just an aggregate the admin
/// list page rolls into the 30-day column).
// sync-audit-only: workspace-local usage counter; no entity sync needed
pub fn record_insertion(
    conn: &mut DbConnection,
    new: NewCannedResponseInsertion,
) -> QueryResult<usize> {
    use crate::schema::canned_response_insertions::dsl::*;
    diesel::insert_into(canned_response_insertions)
        .values(&new)
        .execute(conn)
}

/// Test helper: count rows in the insertions table for one canned
/// response across all time (not the 30-day window). Only compiled
/// in `#[cfg(test)]` so production callers can't accidentally pull
/// the unbounded total instead of the rolling counter.
#[cfg(test)]
pub fn insertion_total_for_test(conn: &mut DbConnection, response_id: i32) -> QueryResult<i64> {
    use crate::schema::canned_response_insertions::dsl::*;
    canned_response_insertions
        .filter(canned_response_id.eq(response_id))
        .count()
        .get_result(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn crud_roundtrip() {
        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "admin", "admin");
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

    /// Inserts get attributed to the right template, and templates
    /// with no insertions surface a zero rather than dropping out
    /// of the list. The latter matters because the admin page sorts
    /// by name by default and would otherwise miss never-used rows.
    #[test]
    fn list_with_insert_counts_zero_when_unused() {
        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "lwic-unused-admin", "admin");
        let row = create(
            &mut conn,
            NewCannedResponse {
                title: "Unused template".into(),
                body: "Body".into(),
                created_by: Some(admin.uuid),
            },
        )
        .unwrap();
        let listed = list_with_insert_counts(&mut conn).unwrap();
        let found = listed
            .iter()
            .find(|r| r.id == row.id)
            .expect("never-used row still listed");
        assert_eq!(found.inserts_30d, 0);
    }

    #[test]
    fn list_with_insert_counts_aggregates_recent_inserts() {
        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "lwic-agg-admin", "admin");
        let row = create(
            &mut conn,
            NewCannedResponse {
                title: "Used template".into(),
                body: "Body".into(),
                created_by: Some(admin.uuid),
            },
        )
        .unwrap();
        for _ in 0..3 {
            record_insertion(
                &mut conn,
                NewCannedResponseInsertion {
                    canned_response_id: row.id,
                    user_uuid: Some(admin.uuid),
                    ticket_id: None,
                    workspace_id: row.workspace_id,
                },
            )
            .unwrap();
        }
        let listed = list_with_insert_counts(&mut conn).unwrap();
        let found = listed.iter().find(|r| r.id == row.id).unwrap();
        assert_eq!(found.inserts_30d, 3);
        assert_eq!(insertion_total_for_test(&mut conn, row.id).unwrap(), 3);
    }
}
