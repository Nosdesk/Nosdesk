use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

// ============================================================================
// Category CRUD Operations
// ============================================================================

/// Get all active categories (for regular users, respects visibility)
pub fn get_all_categories(conn: &mut DbConnection) -> QueryResult<Vec<TicketCategory>> {
    ticket_categories::table
        .filter(ticket_categories::is_active.eq(true))
        .order(ticket_categories::display_order.asc())
        .load(conn)
}

/// Get all categories with visibility information (for admin)
pub fn get_all_categories_with_visibility(conn: &mut DbConnection) -> Result<Vec<CategoryWithVisibility>, Error> {
    let all_categories = ticket_categories::table
        .order(ticket_categories::display_order.asc())
        .load::<TicketCategory>(conn)?;

    let mut categories_with_visibility = Vec::new();

    for category in all_categories {
        let visible_groups = get_visible_groups_for_category(conn, category.id)?;
        let is_public = visible_groups.is_empty();

        categories_with_visibility.push(CategoryWithVisibility {
            category,
            visible_to_groups: visible_groups,
            is_public,
        });
    }

    Ok(categories_with_visibility)
}

/// Get a category by ID
pub fn get_category_by_id(conn: &mut DbConnection, category_id: i32) -> QueryResult<TicketCategory> {
    ticket_categories::table.find(category_id).first(conn)
}

/// Get a category with visibility information
pub fn get_category_with_visibility(conn: &mut DbConnection, category_id: i32) -> Result<CategoryWithVisibility, Error> {
    let category = ticket_categories::table
        .find(category_id)
        .first::<TicketCategory>(conn)?;

    let visible_groups = get_visible_groups_for_category(conn, category_id)?;
    let is_public = visible_groups.is_empty();

    Ok(CategoryWithVisibility {
        category,
        visible_to_groups: visible_groups,
        is_public,
    })
}

/// Create a new category
pub fn create_category(conn: &mut DbConnection, new_category: NewTicketCategory) -> QueryResult<TicketCategory> {
    diesel::insert_into(ticket_categories::table)
        .values(&new_category)
        .get_result(conn)
}

/// Update a category
pub fn update_category(
    conn: &mut DbConnection,
    category_id: i32,
    mut category_update: TicketCategoryUpdate,
) -> QueryResult<TicketCategory> {
    // Set updated_at to current time if not provided
    if category_update.updated_at.is_none() {
        category_update.updated_at = Some(chrono::Utc::now().naive_utc());
    }

    diesel::update(ticket_categories::table.find(category_id))
        .set(&category_update)
        .get_result(conn)
}

/// Soft delete a category (set is_active to false)
pub fn delete_category(conn: &mut DbConnection, category_id: i32) -> QueryResult<TicketCategory> {
    diesel::update(ticket_categories::table.find(category_id))
        .set((
            ticket_categories::is_active.eq(false),
            ticket_categories::updated_at.eq(chrono::Utc::now().naive_utc()),
        ))
        .get_result(conn)
}

/// Get the next display order value
pub fn get_next_display_order(conn: &mut DbConnection) -> QueryResult<i32> {
    let max_order: Option<i32> = ticket_categories::table
        .select(diesel::dsl::max(ticket_categories::display_order))
        .first(conn)?;

    Ok(max_order.unwrap_or(0) + 1)
}

/// Update display orders for categories
pub fn update_category_orders(
    conn: &mut DbConnection,
    orders: Vec<(i32, i32)>, // Vec of (category_id, new_display_order)
) -> QueryResult<()> {
    for (category_id, new_order) in orders {
        diesel::update(ticket_categories::table.find(category_id))
            .set(ticket_categories::display_order.eq(new_order))
            .execute(conn)?;
    }
    Ok(())
}

// ============================================================================
// Category-Group Visibility Operations
// ============================================================================

/// Get groups that can see a category
pub fn get_visible_groups_for_category(conn: &mut DbConnection, category_id: i32) -> QueryResult<Vec<Group>> {
    category_group_visibility::table
        .filter(category_group_visibility::category_id.eq(category_id))
        .inner_join(groups::table)
        .select(groups::all_columns)
        .order(groups::name.asc())
        .load(conn)
}

/// Set which groups can see a category (replaces existing visibility)
pub fn set_category_visibility(
    conn: &mut DbConnection,
    category_id: i32,
    group_ids: Vec<i32>,
    created_by: Option<Uuid>,
) -> QueryResult<Vec<CategoryGroupVisibility>> {
    // Delete all existing visibility entries
    diesel::delete(
        category_group_visibility::table
            .filter(category_group_visibility::category_id.eq(category_id))
    ).execute(conn)?;

    // If no groups specified, the category becomes public (visible to all)
    if group_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Add new visibility entries
    let new_entries: Vec<NewCategoryGroupVisibility> = group_ids
        .iter()
        .map(|group_id| NewCategoryGroupVisibility {
            category_id,
            group_id: *group_id,
            created_by,
        })
        .collect();

    diesel::insert_into(category_group_visibility::table)
        .values(&new_entries)
        .get_results(conn)
}

/// Add a group to category visibility
#[allow(dead_code)]
pub fn add_group_to_category_visibility(
    conn: &mut DbConnection,
    category_id: i32,
    group_id: i32,
    created_by: Option<Uuid>,
) -> QueryResult<CategoryGroupVisibility> {
    // Check if already exists
    let existing = category_group_visibility::table
        .filter(category_group_visibility::category_id.eq(category_id))
        .filter(category_group_visibility::group_id.eq(group_id))
        .first::<CategoryGroupVisibility>(conn);

    if let Ok(visibility) = existing {
        return Ok(visibility);
    }

    let new_entry = NewCategoryGroupVisibility {
        category_id,
        group_id,
        created_by,
    };

    diesel::insert_into(category_group_visibility::table)
        .values(&new_entry)
        .get_result(conn)
}

/// Remove a group from category visibility
#[allow(dead_code)]
pub fn remove_group_from_category_visibility(
    conn: &mut DbConnection,
    category_id: i32,
    group_id: i32,
) -> QueryResult<usize> {
    diesel::delete(
        category_group_visibility::table
            .filter(category_group_visibility::category_id.eq(category_id))
            .filter(category_group_visibility::group_id.eq(group_id))
    ).execute(conn)
}

// ============================================================================
// User-Category Visibility Checks
// ============================================================================

/// Get categories visible to a user based on their group memberships
/// - Admins see all active categories
/// - Regular users see:
///   1. Public categories (no group restrictions)
///   2. Categories where they belong to at least one of the allowed groups
pub fn get_categories_for_user(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    is_admin: bool,
) -> QueryResult<Vec<TicketCategory>> {
    if is_admin {
        // Admins see all active categories
        return get_all_categories(conn);
    }

    // Get user's group IDs
    let user_group_ids: Vec<i32> = crate::repository::groups::get_group_ids_for_user(conn, user_uuid)?;

    // Get all active categories
    let all_categories = ticket_categories::table
        .filter(ticket_categories::is_active.eq(true))
        .order(ticket_categories::display_order.asc())
        .load::<TicketCategory>(conn)?;

    // Filter by visibility
    let mut visible_categories = Vec::new();

    for category in all_categories {
        // Get group IDs that can see this category
        let category_group_ids: Vec<i32> = category_group_visibility::table
            .filter(category_group_visibility::category_id.eq(category.id))
            .select(category_group_visibility::group_id)
            .load(conn)?;

        // If no groups specified, category is public
        if category_group_ids.is_empty() {
            visible_categories.push(category);
            continue;
        }

        // Check if user is in any of the allowed groups
        let has_access = user_group_ids.iter().any(|id| category_group_ids.contains(id));
        if has_access {
            visible_categories.push(category);
        }
    }

    Ok(visible_categories)
}

/// Check if a user can see a specific category
pub fn can_user_see_category(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    category_id: i32,
    is_admin: bool,
) -> QueryResult<bool> {
    if is_admin {
        return Ok(true);
    }

    // Get group IDs that can see this category
    let category_group_ids: Vec<i32> = category_group_visibility::table
        .filter(category_group_visibility::category_id.eq(category_id))
        .select(category_group_visibility::group_id)
        .load(conn)?;

    // If no groups specified, category is public
    if category_group_ids.is_empty() {
        return Ok(true);
    }

    // Get user's group IDs
    let user_group_ids: Vec<i32> = crate::repository::groups::get_group_ids_for_user(conn, user_uuid)?;

    // Check if user is in any of the allowed groups
    Ok(user_group_ids.iter().any(|id| category_group_ids.contains(id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use crate::models::UserRole;

    #[test]
    fn public_category_visible_to_any_user() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "alice", UserRole::User);
        let cat = TestFixtures::create_category(&mut conn, "Public");
        // No group restrictions → public
        assert!(can_user_see_category(&mut conn, &user.uuid, cat.id, false).unwrap());
    }

    #[test]
    fn restricted_category_visible_to_allowed_group() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "bob", UserRole::User);
        let group = TestFixtures::create_group(&mut conn, "Support");
        TestFixtures::add_user_to_group(&mut conn, user.uuid, group.id);
        let cat = TestFixtures::create_category(&mut conn, "VIP");
        TestFixtures::set_category_visibility(&mut conn, cat.id, &[group.id]);

        assert!(can_user_see_category(&mut conn, &user.uuid, cat.id, false).unwrap());
    }

    #[test]
    fn restricted_category_hidden_from_non_member() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "carol", UserRole::User);
        let group = TestFixtures::create_group(&mut conn, "VIP Group");
        // user is NOT added to the group
        let cat = TestFixtures::create_category(&mut conn, "VIP Only");
        TestFixtures::set_category_visibility(&mut conn, cat.id, &[group.id]);

        assert!(!can_user_see_category(&mut conn, &user.uuid, cat.id, false).unwrap());
    }

    #[test]
    fn admin_sees_restricted_category() {
        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "admin", UserRole::Admin);
        let group = TestFixtures::create_group(&mut conn, "Secret");
        let cat = TestFixtures::create_category(&mut conn, "Secret Cat");
        TestFixtures::set_category_visibility(&mut conn, cat.id, &[group.id]);
        // admin not in group but passes is_admin=true
        assert!(can_user_see_category(&mut conn, &admin.uuid, cat.id, true).unwrap());
    }

    #[test]
    fn get_categories_returns_public_and_accessible() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "dave", UserRole::User);
        let group = TestFixtures::create_group(&mut conn, "Eng");
        TestFixtures::add_user_to_group(&mut conn, user.uuid, group.id);

        let public_cat = TestFixtures::create_category(&mut conn, "Public Cat");
        let restricted_ok = TestFixtures::create_category(&mut conn, "Eng Cat");
        TestFixtures::set_category_visibility(&mut conn, restricted_ok.id, &[group.id]);
        let other_group = TestFixtures::create_group(&mut conn, "Finance");
        let restricted_no = TestFixtures::create_category(&mut conn, "Finance Cat");
        TestFixtures::set_category_visibility(&mut conn, restricted_no.id, &[other_group.id]);

        let visible = get_categories_for_user(&mut conn, &user.uuid, false).unwrap();
        let visible_ids: Vec<i32> = visible.iter().map(|c| c.id).collect();

        assert!(visible_ids.contains(&public_cat.id));
        assert!(visible_ids.contains(&restricted_ok.id));
        assert!(!visible_ids.contains(&restricted_no.id));
    }
}
