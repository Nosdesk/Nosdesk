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
