use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ===== USER RECOVERY CODES =====

/// One row of `user_recovery_codes`. Each MFA backup/recovery code
/// is stored as its own row keyed by a `BIGSERIAL` id, with the
/// hash opaque and a nullable `used_at` recording the moment a
/// successful verify consumed it.
///
/// The atomicity invariant is held by Postgres, not the app:
/// consumption is a single `UPDATE … WHERE id = $1 AND used_at IS
/// NULL RETURNING …` so two concurrent verifies racing the same
/// code resolve to one succeeded, one failed at the row-level
/// lock. The earlier JSONB-array design forced a read-modify-write
/// of the full array per consumption and lost concurrent
/// consumptions to last-write-wins.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::user_recovery_codes)]
pub struct UserRecoveryCode {
    pub id: i64,
    pub user_uuid: Uuid,
    pub code_hash: String,
    pub used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::user_recovery_codes)]
pub struct NewUserRecoveryCode {
    pub user_uuid: Uuid,
    pub code_hash: String,
}
