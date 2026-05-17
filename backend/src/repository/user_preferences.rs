//! Repository for the `user_preferences` table.
//!
//! Split out of `users` on 2026-05-14 to keep that table narrow
//! and group related concerns. A row exists for every user — the
//! `trg_users_auto_create_preferences` trigger on `users` ensures
//! that, so consumers don't have to handle "no row yet" and the
//! application code never needs an INSERT-after-create-user
//! step. FK is `ON DELETE CASCADE`, so user deletion cleans up.
//!
//! Resolution chain for locale / timezone:
//!   `user_preferences.locale` (non-NULL)
//!     → `site_settings.default_locale`
//!     → hardcoded `'en-US'`
//! Same shape for timezone. The hardcoded fallback is the
//! belt-and-braces case; `site_settings` always has a row with
//! the NOT-NULL-with-default columns added in
//! `2026-05-14-100000_add_locale_and_timezone`.

use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{UpdateUserPreferences, UserPreferences};
use crate::schema::user_preferences;

/// Fetch the preferences row for a user. The trigger guarantees
/// the row exists, so this is `Result<UserPreferences, _>` not
/// `Result<Option<UserPreferences>, _>` — if you get
/// `Error::NotFound` from this, the trigger has failed or the
/// user was deleted out from under you.
pub fn get(
    conn: &mut DbConnection,
    user_uuid: uuid::Uuid,
) -> Result<UserPreferences, diesel::result::Error> {
    user_preferences::table.find(user_uuid).first(conn)
}

/// Batch-fetch preferences for many users. Used by list endpoints
/// that hand the result to `repository::user_helpers::get_users_with_primary_emails`
/// for the flattened response shape; one query instead of N+1.
pub fn get_many(
    conn: &mut DbConnection,
    user_uuids: &[uuid::Uuid],
) -> Result<Vec<UserPreferences>, diesel::result::Error> {
    user_preferences::table
        .filter(user_preferences::user_uuid.eq_any(user_uuids))
        .load(conn)
}

// sync-audit-only: per-user UI preferences; not observed by other clients
/// Patch one user's preferences. The `Option<Option<T>>` shape
/// of `UpdateUserPreferences` means:
///   - outer `None`        → leave the column alone
///   - `Some(None)`        → clear the column (revert to site default)
///   - `Some(Some(value))` → set the column
/// Diesel's `AsChangeset` derive handles the column-set machinery;
/// `updated_at` is stamped automatically by the existing
/// `set_updated_at` trigger.
pub fn update(
    conn: &mut DbConnection,
    user_uuid: uuid::Uuid,
    changes: UpdateUserPreferences,
) -> Result<UserPreferences, diesel::result::Error> {
    diesel::update(user_preferences::table.find(user_uuid))
        .set(&changes)
        .get_result(conn)
}

/// Read just the signature column. Hot path: outbound channel
/// replies fetch only this one field per outgoing message.
/// Returns `None` when the column is NULL (no signature
/// configured); callers append nothing in that case.
pub fn get_signature(
    conn: &mut DbConnection,
    user_uuid: uuid::Uuid,
) -> Result<Option<String>, diesel::result::Error> {
    user_preferences::table
        .find(user_uuid)
        .select(user_preferences::signature)
        .first(conn)
}
