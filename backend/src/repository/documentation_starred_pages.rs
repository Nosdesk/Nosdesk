use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{DocumentationStarredPage, NewDocumentationStarredPage, StarredPageInfo};
use crate::schema::{documentation_pages, documentation_starred_pages};

/// Get all starred pages for a user, with page metadata, ordered by most recently starred
pub fn get_user_starred_pages(conn: &mut DbConnection, user_uuid: Uuid) -> Vec<StarredPageInfo> {
    documentation_starred_pages::table
        .inner_join(
            documentation_pages::table
                .on(documentation_pages::id.eq(documentation_starred_pages::page_id)),
        )
        .filter(documentation_starred_pages::user_uuid.eq(user_uuid))
        .filter(documentation_pages::deleted_at.is_null())
        .select((
            documentation_starred_pages::page_id,
            documentation_pages::title,
            documentation_pages::slug,
            documentation_pages::icon,
            documentation_starred_pages::created_at,
        ))
        .order(documentation_starred_pages::created_at.desc())
        .load::<(
            i32,
            String,
            String,
            Option<String>,
            chrono::DateTime<chrono::Utc>,
        )>(conn)
        .unwrap_or_default()
        .into_iter()
        .map(|(page_id, title, slug, icon, starred_at)| StarredPageInfo {
            page_id,
            title,
            slug,
            icon,
            starred_at,
        })
        .collect()
}

/// Check if a specific page is starred by a user
pub fn is_page_starred(conn: &mut DbConnection, user_uuid: Uuid, page_id: i32) -> bool {
    documentation_starred_pages::table
        .filter(documentation_starred_pages::user_uuid.eq(user_uuid))
        .filter(documentation_starred_pages::page_id.eq(page_id))
        .count()
        .get_result::<i64>(conn)
        .unwrap_or(0)
        > 0
}

/// Star a page for a user
pub fn star_page(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    page_id: i32,
) -> Result<DocumentationStarredPage, diesel::result::Error> {
    let new_star = NewDocumentationStarredPage { user_uuid, page_id };
    diesel::insert_into(documentation_starred_pages::table)
        .values(&new_star)
        .on_conflict((
            documentation_starred_pages::user_uuid,
            documentation_starred_pages::page_id,
        ))
        .do_nothing()
        .execute(conn)?;

    // Return the starred page (may have already existed)
    documentation_starred_pages::table
        .filter(documentation_starred_pages::user_uuid.eq(user_uuid))
        .filter(documentation_starred_pages::page_id.eq(page_id))
        .first(conn)
}

/// Unstar a page for a user
pub fn unstar_page(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    page_id: i32,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(
        documentation_starred_pages::table
            .filter(documentation_starred_pages::user_uuid.eq(user_uuid))
            .filter(documentation_starred_pages::page_id.eq(page_id)),
    )
    .execute(conn)
}
