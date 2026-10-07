//! Resolve a user's effective locale from the database.
//!
//! Walks the same chain as `utils::locale::effective_locale`,
//! reading both the user's stored preference and the site default
//! in a single helper so call sites (transactional email,
//! notification dispatch, outbound channel replies) don't each
//! re-implement the same two queries.
//!
//! All DB errors are best-effort: a missing user_preferences row,
//! a missing site_settings row, or a parse failure all fall
//! through to `DEFAULT_LOCALE` rather than surfacing an error. A
//! locale lookup should never sink the request it's part of.

use diesel::prelude::*;
use unic_langid::LanguageIdentifier;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::utils::locale::effective_locale;

/// Look up the effective locale for `user_uuid`: their own choice, else the
/// default of the workspace the connection is pinned to. See module doc.
pub fn resolve_effective_locale(conn: &mut DbConnection, user_uuid: Uuid) -> LanguageIdentifier {
    use crate::schema::site_settings;

    let user_pref = user_locale_preference(conn, user_uuid);

    // Filtered on the pin rather than left to row security, so an elevated
    // caller reads the same row; an unpinned connection reads none and falls
    // through to the default.
    let site_default: String = site_settings::table
        .filter(site_settings::workspace_id.eq(crate::repository::pinned_workspace()))
        .select(site_settings::default_locale)
        .first::<String>(conn)
        .unwrap_or_default();

    effective_locale(user_pref.as_deref(), &site_default)
}

/// The locale `user_uuid` chose for themselves, if any. Callers that already
/// hold their workspace's settings pair it with `default_locale` through
/// `utils::locale::effective_locale`.
pub fn user_locale_preference(conn: &mut DbConnection, user_uuid: Uuid) -> Option<String> {
    use crate::schema::user_preferences;

    user_preferences::table
        .find(user_uuid)
        .select(user_preferences::locale)
        .first::<Option<String>>(conn)
        .optional()
        .ok()
        .flatten()
        .flatten()
}
