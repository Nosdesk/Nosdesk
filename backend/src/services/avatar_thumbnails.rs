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
use diesel::sql_types::Text;

use crate::db::DbConnection;
use crate::utils::image::generate_user_avatar_thumbnail;

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
    #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
    avatar_thumb: Option<String>,
}

/// Regenerate avatar thumbnails for users with an avatar, writing the
/// resulting path back to `users.avatar_thumb`. Best-effort: a single
/// user's failure is logged and the run moves on.
///
/// Connection note: this runs synchronous Diesel queries before and
/// after the async image work, holding `conn` across the `.await`s.
/// Callers invoke it inline from an async handler, via a one-shot
/// runtime (`nosdesk-cli`), or from the scheduler with an owned,
/// per-tick pooled connection.
pub async fn backfill_thumbnails(conn: &mut DbConnection, mode: BackfillMode) -> BackfillStats {
    let rows: Vec<AvatarRow> = match diesel::sql_query(
        "SELECT uuid::text AS uuid_str, avatar_url, avatar_thumb \
         FROM users WHERE avatar_url IS NOT NULL",
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
        if mode == BackfillMode::MissingOnly && !needs_regeneration(&row) {
            continue;
        }
        stats.checked += 1;

        match generate_user_avatar_thumbnail(&row.avatar_url, &row.uuid_str).await {
            Ok(Some(thumb_url)) => {
                // Persist the path so the column matches the file we
                // just wrote. The old handler-local routine skipped this
                // and only worked because the path is deterministic;
                // users with a NULL/stale `avatar_thumb` stayed broken.
                match persist_thumb(conn, &row.uuid_str, &thumb_url) {
                    Ok(()) => stats.regenerated += 1,
                    Err(e) => {
                        stats.failed += 1;
                        tracing::warn!(user = %row.uuid_str, error = %e, "thumbnail backfill: db write-back failed");
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

/// A row needs regeneration when its column is unset or the file it
/// points at is gone. The deterministic thumb path mirrors the one
/// [`generate_user_avatar_thumbnail`] writes, so a plain existence
/// check is enough.
fn needs_regeneration(row: &AvatarRow) -> bool {
    if row.avatar_thumb.as_deref().is_none_or(str::is_empty) {
        return true;
    }
    !thumb_file_exists(&row.uuid_str)
}

fn thumb_file_exists(uuid: &str) -> bool {
    std::path::Path::new(&format!("uploads/users/thumbs/{uuid}_thumb.webp")).exists()
}

fn persist_thumb(
    conn: &mut DbConnection,
    uuid_str: &str,
    thumb_url: &str,
) -> Result<(), diesel::result::Error> {
    diesel::sql_query("UPDATE users SET avatar_thumb = $1 WHERE uuid = $2::uuid")
        .bind::<Text, _>(thumb_url)
        .bind::<Text, _>(uuid_str)
        .execute(conn)?;
    Ok(())
}
