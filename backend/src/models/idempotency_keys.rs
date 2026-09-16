use diesel::prelude::*;

// ===== IDEMPOTENCY KEY MODELS =====

/// Cached response for a previously-seen `Idempotency-Key` header.
/// Looked up by the Idempotency middleware (M5 Task 2) to short-
/// circuit retries on POST / PUT / PATCH callbacks from the
/// control-plane provisioning worker.
#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::idempotency_keys)]
#[diesel(primary_key(key))]
pub struct IdempotencyRecord {
    pub key: String,
    pub response_body: serde_json::Value,
    pub response_status: i16,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::idempotency_keys)]
pub struct NewIdempotencyRecord {
    pub key: String,
    pub response_body: serde_json::Value,
    pub response_status: i16,
}
