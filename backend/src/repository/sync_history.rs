use diesel::prelude::*;
use chrono::Utc;
use crate::db::DbConnection;
use crate::models::{SyncHistory, NewSyncHistory, SyncHistoryUpdate};
use crate::schema::sync_history;

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

 