//! Typed access to the `system_meta` key/value bookkeeping table.
//!
//! Three keys are seeded by the substrate migration:
//! - `schema_hash` (`String`): the binary's compiled schema hash; the
//!   server writes its current hash on boot and clients use it to
//!   detect mismatches against an out-of-date bootstrap.
//! - `sync_id_high_water` (`i64`): mostly informational; the
//!   delta handler uses MAX(sync_id) directly from `sync_actions`
//!   for cursors.
//! - `partition_max_provisioned` (`String`): an ISO date naming the
//!   first month NOT yet partitioned. The partition-provisioning
//!   task uses this to know whether more months need creating.

use diesel::prelude::*;
use diesel::sql_types::{Jsonb, Text};
use serde_json::Value;

use crate::db::DbConnection;
use crate::schema::system_meta;

pub const KEY_SCHEMA_HASH: &str = "schema_hash";
pub const KEY_SYNC_ID_HIGH_WATER: &str = "sync_id_high_water";
pub const KEY_PARTITION_MAX_PROVISIONED: &str = "partition_max_provisioned";

pub fn get(conn: &mut DbConnection, key: &str) -> QueryResult<Option<Value>> {
    system_meta::table
        .find(key)
        .select(system_meta::value)
        .first::<Value>(conn)
        .optional()
}

/// Upsert a key. Updates `updated_at` on every write.
pub fn put(conn: &mut DbConnection, key: &str, value: &Value) -> QueryResult<()> {
    diesel::sql_query(
        "INSERT INTO system_meta (key, value, updated_at) \
         VALUES ($1, $2, NOW()) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW()",
    )
    .bind::<Text, _>(key)
    .bind::<Jsonb, _>(value)
    .execute(conn)?;
    Ok(())
}

/// Convenience: read `schema_hash` as a String. Returns empty string
/// when unset (fresh install before the boot writer runs).
pub fn schema_hash(conn: &mut DbConnection) -> QueryResult<String> {
    Ok(get(conn, KEY_SCHEMA_HASH)?
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default())
}

/// Write the binary's compiled schema hash. Call once during boot.
pub fn set_schema_hash(conn: &mut DbConnection, hash: &str) -> QueryResult<()> {
    put(conn, KEY_SCHEMA_HASH, &Value::String(hash.to_string()))
}
