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

/// Look up the effective locale for `user_uuid`. See module doc.
pub fn resolve_effective_locale(
    conn: &mut DbConnection,
    user_uuid: Uuid,
) -> LanguageIdentifier {
    use crate::schema::{site_settings, user_preferences};

    let user_pref: Option<String> = user_preferences::table
        .find(user_uuid)
        .select(user_preferences::locale)
        .first::<Option<String>>(conn)
        .optional()
        .ok()
        .flatten()
        .flatten();

    let site_default: String = site_settings::table
        .find(1)
        .select(site_settings::default_locale)
        .first::<String>(conn)
        .unwrap_or_default();

    effective_locale(user_pref.as_deref(), &site_default)
}
