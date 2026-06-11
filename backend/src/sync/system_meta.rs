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
pub const KEY_INSTANCE_ID: &str = "instance_id";

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

/// Read the database's instance id (empty string when unset).
///
/// This is a random UUID minted once per database by
/// [`ensure_instance_id`]. It is stable for the life of the database
/// and only changes when the database is freshly initialised (the row
/// is gone), so clients use it as an "epoch" fence: a change means the
/// cached local data belongs to a different database generation and
/// must be wiped. See `docs/plans/collab-stale-cache-fence.md`.
pub fn instance_id(conn: &mut DbConnection) -> QueryResult<String> {
    Ok(get(conn, KEY_INSTANCE_ID)?
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default())
}

/// Mint the database instance id if it isn't set yet, then return it.
/// Insert-if-absent (never overwrites), so the value is stable across
/// boots and is only regenerated when the database is recreated. Call
/// once during boot, after migrations.
pub fn ensure_instance_id(conn: &mut DbConnection) -> QueryResult<String> {
    let new_id = uuid::Uuid::new_v4().to_string();
    diesel::sql_query(
        "INSERT INTO system_meta (key, value, updated_at) \
         VALUES ($1, $2, NOW()) \
         ON CONFLICT (key) DO NOTHING",
    )
    .bind::<Text, _>(KEY_INSTANCE_ID)
    .bind::<Jsonb, _>(Value::String(new_id))
    .execute(conn)?;
    instance_id(conn)
}
