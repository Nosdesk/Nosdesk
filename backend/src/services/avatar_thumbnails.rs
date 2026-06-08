//! Reusable, idempotent avatar-thumbnail backfill.
//!
//! Profile thumbnails (`users.avatar_thumb`, a 48x48 WebP) are derived
//! from the full avatar (`users.avatar_url`) and live at a deterministic
//! path: `uploads/users/thumbs/{uuid}_thumb.webp`. Backups carry the
//! avatar original but deliberately skip the thumbs directory ("cheap to
//! regenerate"), so after any restore the thumb *files* are gone even
//! though `avatar_thumb` still references them.
//!
//! This module owns the single regeneration routine, so every code path
//! that needs it shares one implementation rather than diverging:
//!   * admin HTTP restore (`handlers::backup::execute_restore`)
//!   * `nosdesk-cli db restore`
//!   * the daily `users.backfill_thumbnails` scheduled job
//!
//! It is idempotent: [`BackfillMode::MissingOnly`] regenerates only
//! where the thumb file is absent or `avatar_thumb` is unset, so the
//! periodic sweep does no work in steady state.

use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text};

use crate::db::DbConnection;
use crate::sync::actor::ActorContext;
use crate::sync::session::with_actor_context;
use crate::utils::image::generate_user_avatar_thumbnail;
use crate::utils::storage::process_storage;

/// Whether to regenerate every avatar's thumbnail or only the missing
/// ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillMode {
    /// Regenerate for every user with an avatar. Used right after a
    /// restore, where the thumbs directory was not part of the backup
    /// so every thumb file needs rebuilding regardless of column state.
    Force,
    /// Regenerate only where the thumb file is missing on disk or
    /// `avatar_thumb` is NULL. Used by the periodic safety-net sweep so
    /// it costs nothing once everything is in place.
    MissingOnly,
}

/// Outcome counts for a backfill run.
#[derive(Debug, Default, Clone, Copy)]
pub struct BackfillStats {
    /// Users whose thumbnail we attempted to (re)generate.
    pub checked: usize,
    /// Thumbnails (re)generated and written back to `avatar_thumb`.
    pub regenerated: usize,
    /// Attempts that failed (avatar file missing, decode error, db
    /// write-back failed, …). Logged per-user; the run continues.
    pub failed: usize,
}

#[derive(QueryableByName)]
struct AvatarRow {
    #[diesel(sql_type = Text)]
    uuid_str: String,
    #[diesel(sql_type = Text)]
    avatar_url: String,
    #[diesel(sql_type = Nullable<Text>)]
    avatar_thumb: Option<String>,
    /// A workspace the user belongs to, used only to attribute the
    /// audit_log row when `avatar_thumb` actually has to change. NULL
    /// when the user has no membership (then the column write is
    /// skipped: the file is still regenerated).
    #[diesel(sql_type = Nullable<Integer>)]
    workspace_id: Option<i32>,
}

/// Regenerate avatar thumbnails for users with an avatar. The thumbnail
/// file is rewritten at its deterministic path; `users.avatar_thumb` is
/// updated only when it actually changes (NULL or a different path).
/// Best-effort: a single user's failure is logged and the run moves on.
///
/// `reference` is the audit/sync actor reference recorded on any
/// `avatar_thumb` write (see `sync::session` for the prefix
/// convention), so operators can tell the scheduler sweep, the restore
/// paths, and the admin button apart in the audit log.
///
/// Connection note: synchronous Diesel queries bracket the async image
/// work, holding `conn` across the `.await`s. Callers invoke it inline
/// from an async handler, via a one-shot runtime (`nosdesk-cli`), or
/// from the scheduler with an owned, per-tick pooled connection.
pub async fn backfill_thumbnails(
    conn: &mut DbConnection,
    mode: BackfillMode,
    reference: &'static str,
) -> BackfillStats {
    // Resolve one workspace per user up front so the (rare) column write
    // can pin `app.workspace_id`; the audited `users` write otherwise
    // trips the audit context guard. The `ORDER BY workspace_id LIMIT 1`
    // pick is deterministic but arbitrary for a user who belongs to more
    // than one workspace (only possible under hosted multi-tenancy): the
    // thumbnail backfill is per-user, not per-workspace, so the audit row
    // is attributed to the user's lowest-id workspace. Acceptable until
    // hosted attribution requirements firm up.
    let rows: Vec<AvatarRow> = match diesel::sql_query(
        "SELECT u.uuid::text AS uuid_str, u.avatar_url, u.avatar_thumb, \
                (SELECT wm.workspace_id FROM workspace_members wm \
                 WHERE wm.user_uuid = u.uuid ORDER BY wm.workspace_id LIMIT 1) AS workspace_id \
         FROM users u WHERE u.avatar_url IS NOT NULL",
    )
    .load(conn)
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(error = %e, "thumbnail backfill: failed to query users");
            return BackfillStats::default();
        }
    };

    let mut stats = BackfillStats::default();

    for row in rows {
        if mode == BackfillMode::MissingOnly && !needs_regeneration(&row).await {
            continue;
        }
        stats.checked += 1;

        match generate_user_avatar_thumbnail(&row.avatar_url, &row.uuid_str).await {
            Ok(Some(thumb_url)) => {
                // The path is deterministic, so a regenerated file usually
                // matches the column already (every restore case): skip the
                // write entirely and avoid the audited-write workspace dance.
                if row.avatar_thumb.as_deref() == Some(thumb_url.as_str()) {
                    stats.regenerated += 1;
                } else {
                    match persist_thumb(conn, &row, &thumb_url, reference) {
                        Ok(()) => stats.regenerated += 1,
                        Err(e) => {
                            stats.failed += 1;
                            tracing::warn!(user = %row.uuid_str, error = %e, "thumbnail backfill: db write-back failed");
                        }
                    }
                }
            }
            Ok(None) => {
                stats.failed += 1;
                tracing::warn!(user = %row.uuid_str, "thumbnail backfill: avatar file missing or undecodable");
            }
            Err(e) => {
                stats.failed += 1;
                tracing::warn!(user = %row.uuid_str, error = %e, "thumbnail backfill: generation failed");
            }
        }
    }

    stats
}

/// A row needs regeneration when its column is unset or the object it
/// points at is gone. The deterministic thumb path mirrors the one
/// [`generate_user_avatar_thumbnail`] writes, so a plain existence
/// check is enough. The check goes through the storage backend so it's
/// correct for both local disk and S3/Tigris.
async fn needs_regeneration(row: &AvatarRow) -> bool {
    if row.avatar_thumb.as_deref().is_none_or(str::is_empty) {
        return true;
    }
    !thumb_file_exists(&row.uuid_str).await
}

async fn thumb_file_exists(uuid: &str) -> bool {
    let path = format!("users/thumbs/{uuid}_thumb.webp");
    process_storage().file_exists(&path).await.unwrap_or(false)
}

/// Write `avatar_thumb`, attributing the audited change to the user's
/// workspace. `users` carries an audit_log trigger whose row requires a
/// non-null `app.workspace_id`; pin the user's workspace so the write
/// succeeds (a plain or BYPASSRLS connection without the GUC violates
/// the audit_log NOT NULL constraint). With no membership there's
/// nothing to attribute to, so skip the column write — the file was
/// still regenerated.
fn persist_thumb(
    conn: &mut DbConnection,
    row: &AvatarRow,
    thumb_url: &str,
    reference: &'static str,
) -> Result<(), diesel::result::Error> {
    let Some(workspace_id) = row.workspace_id else {
        tracing::warn!(
            user = %row.uuid_str,
            "thumbnail backfill: no workspace to attribute avatar_thumb write; file regenerated, column left unchanged"
        );
        return Ok(());
    };

    let actor = ActorContext::system(reference).with_workspace(workspace_id);
    with_actor_context(conn, &actor, |conn| {
        diesel::sql_query("UPDATE users SET avatar_thumb = $1 WHERE uuid = $2::uuid")
            .bind::<Text, _>(thumb_url)
            .bind::<Text, _>(&row.uuid_str)
            .execute(conn)?;
        Ok::<(), diesel::result::Error>(())
    })
}
