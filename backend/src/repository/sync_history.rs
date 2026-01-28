use diesel::prelude::*;
use crate::db::DbConnection;
use crate::models::{SyncHistory, NewSyncHistory, SyncHistoryUpdate, SyncDeltaToken, NewSyncDeltaToken};
use crate::schema::{sync_history, sync_delta_tokens};

/// Create a new sync history record
pub fn create_sync_history(
    conn: &mut DbConnection,
    new_sync: NewSyncHistory,
) -> QueryResult<SyncHistory> {
    diesel::insert_into(sync_history::table)
        .values(&new_sync)
        .get_result(conn)
}

/// Update an existing sync history record
pub fn update_sync_history(
    conn: &mut DbConnection,
    sync_id: i32,
    update: SyncHistoryUpdate,
) -> QueryResult<SyncHistory> {
    diesel::update(sync_history::table.find(sync_id))
        .set(&update)
        .get_result(conn)
}

/// Get the most recent completed sync
pub fn get_last_completed_sync(
    conn: &mut DbConnection,
) -> QueryResult<SyncHistory> {
    sync_history::table
        .filter(
            sync_history::status.eq("completed")
                .or(sync_history::status.eq("error"))
                .or(sync_history::status.eq("cancelled"))
        )
        .order(sync_history::started_at.desc())
        .first(conn)
}

// ============================================================================
// Delta Token Operations (for incremental sync)
// ============================================================================

/// Get a delta token for a specific provider and entity type
pub fn get_delta_token(
    conn: &mut DbConnection,
    provider_type: &str,
    entity_type: &str,
) -> QueryResult<SyncDeltaToken> {
    sync_delta_tokens::table
        .filter(sync_delta_tokens::provider_type.eq(provider_type))
        .filter(sync_delta_tokens::entity_type.eq(entity_type))
        .first(conn)
}

/// Save or update a delta token (upsert)
pub fn upsert_delta_token(
    conn: &mut DbConnection,
    provider_type: &str,
    entity_type: &str,
    delta_link: &str,
) -> QueryResult<SyncDeltaToken> {
    use diesel::dsl::now;

    // Try to find existing token
    let existing = sync_delta_tokens::table
        .filter(sync_delta_tokens::provider_type.eq(provider_type))
        .filter(sync_delta_tokens::entity_type.eq(entity_type))
        .first::<SyncDeltaToken>(conn);

    match existing {
        Ok(token) => {
            // Update existing token
            diesel::update(sync_delta_tokens::table.find(token.id))
                .set((
                    sync_delta_tokens::delta_link.eq(delta_link),
                    sync_delta_tokens::updated_at.eq(now),
                ))
                .get_result(conn)
        }
        Err(diesel::result::Error::NotFound) => {
            // Create new token
            let new_token = NewSyncDeltaToken {
                provider_type: provider_type.to_string(),
                entity_type: entity_type.to_string(),
                delta_link: delta_link.to_string(),
            };

            diesel::insert_into(sync_delta_tokens::table)
                .values(&new_token)
                .get_result(conn)
        }
        Err(e) => Err(e),
    }
}

/// Delete a delta token (forces full sync next time)
pub fn delete_delta_token(
    conn: &mut DbConnection,
    provider_type: &str,
    entity_type: &str,
) -> QueryResult<usize> {
    diesel::delete(
        sync_delta_tokens::table
            .filter(sync_delta_tokens::provider_type.eq(provider_type))
            .filter(sync_delta_tokens::entity_type.eq(entity_type))
    )
    .execute(conn)
}

/// Delete all delta tokens for a provider (forces full sync for all entities)
#[allow(dead_code)]
pub fn delete_all_delta_tokens_for_provider(
    conn: &mut DbConnection,
    provider_type: &str,
) -> QueryResult<usize> {
    diesel::delete(
        sync_delta_tokens::table
            .filter(sync_delta_tokens::provider_type.eq(provider_type))
    )
    .execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;
    use chrono::Utc;

    #[test]
    fn create_sync_history_test() {
        let mut conn = setup_test_connection();
        let now = Utc::now().naive_utc();

        let new_sync = NewSyncHistory {
            sync_type: "full".to_string(),
            status: "running".to_string(),
            started_at: now,
            completed_at: None,
            error_message: None,
            records_processed: None,
            records_created: None,
            records_updated: None,
            records_failed: None,
            tenant_id: Some("tenant-1".to_string()),
            is_delta: false,
        };

        let record = create_sync_history(&mut conn, new_sync).unwrap();
        assert_eq!(record.sync_type, "full");
        assert_eq!(record.status, "running");
        assert_eq!(record.tenant_id, Some("tenant-1".to_string()));
        assert!(!record.is_delta);
    }

    #[test]
    fn upsert_and_get_delta_token() {
        let mut conn = setup_test_connection();

        let token = upsert_delta_token(&mut conn, "microsoft", "users", "https://delta.link/1").unwrap();
        assert_eq!(token.provider_type, "microsoft");
        assert_eq!(token.entity_type, "users");
        assert_eq!(token.delta_link, "https://delta.link/1");

        // Upsert again with new link
        let updated = upsert_delta_token(&mut conn, "microsoft", "users", "https://delta.link/2").unwrap();
        assert_eq!(updated.id, token.id);
        assert_eq!(updated.delta_link, "https://delta.link/2");

        // Get it back
        let fetched = get_delta_token(&mut conn, "microsoft", "users").unwrap();
        assert_eq!(fetched.delta_link, "https://delta.link/2");
    }

    #[test]
    fn delete_delta_token_test() {
        let mut conn = setup_test_connection();

        upsert_delta_token(&mut conn, "microsoft", "groups", "https://delta.link/g").unwrap();
        let deleted = delete_delta_token(&mut conn, "microsoft", "groups").unwrap();
        assert_eq!(deleted, 1);

        assert!(get_delta_token(&mut conn, "microsoft", "groups").is_err());
    }
}
