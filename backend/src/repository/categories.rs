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
pub fn get_all_categories_with_visibility(
    conn: &mut DbConnection,
) -> Result<Vec<CategoryWithVisibility>, Error> {
    let all_categories = ticket_categories::table
        .order(ticket_categories::display_order.asc())
        .load::<TicketCategory>(conn)?;

    let ids: Vec<i32> = all_categories.iter().map(|c| c.id).collect();
    let mut groups_map = get_visible_groups_for_categories(conn, &ids)?;

    Ok(all_categories
        .into_iter()
        .map(|category| {
            let visible_groups = groups_map.remove(&category.id).unwrap_or_default();
            let is_public = visible_groups.is_empty();
            CategoryWithVisibility {
                category,
                visible_to_groups: visible_groups,
                is_public,
            }
        })
        .collect())
}

/// Get a category by ID
pub fn get_category_by_id(
    conn: &mut DbConnection,
    category_id: i32,
) -> QueryResult<TicketCategory> {
    ticket_categories::table.find(category_id).first(conn)
}

/// Get a category with visibility information
pub fn get_category_with_visibility(
    conn: &mut DbConnection,
    category_id: i32,
) -> Result<CategoryWithVisibility, Error> {
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

// sync-pending-wire: needs sync aggregate wiring
/// Create a new category
pub fn create_category(
    conn: &mut DbConnection,
    new_category: NewTicketCategory,
) -> QueryResult<TicketCategory> {
    diesel::insert_into(ticket_categories::table)
        .values(&new_category)
        .get_result(conn)
}

// sync-pending-wire: needs sync aggregate wiring
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

// sync-pending-wire: needs sync aggregate wiring
/// Soft delete a category (set is_active to false)
pub fn delete_category(conn: &mut DbConnection, category_id: i32) -> QueryResult<TicketCategory> {
    diesel::update(ticket_categories::table.find(category_id))
        .set((
            ticket_categories::is_active.eq(false),
            ticket_categories::updated_at.eq(chrono::Utc::now().naive_utc()),
        ))
        .get_result(conn)
}

// sync-audit-only: first-boot bootstrap, not a user-driven write
/// First-run seeder: insert a small starter set of categories so a
/// fresh install isn't faced with an empty category dropdown when
/// creating the first ticket. No-ops if any rows exist (regardless
/// of `is_active`) so re-running setup never trashes admin edits or
/// doubles up the defaults. The UNIQUE constraint on `name` plus
/// `ON CONFLICT DO NOTHING` makes a concurrent setup race idempotent
/// at the database level too — losing INSERTs are dropped silently
/// rather than producing duplicate rows.
///
/// Icon names map to the frontend icon registry in
/// `CategoriesManagementView.vue::iconOptions` — keep them in sync
/// when the registry changes.
pub fn seed_defaults_if_empty(
    conn: &mut DbConnection,
    created_by: Option<Uuid>,
) -> QueryResult<usize> {
    use diesel::dsl::count_star;

    let existing: i64 = ticket_categories::table.select(count_star()).first(conn)?;
    if existing > 0 {
        return Ok(0);
    }

    let defaults = [
        (
            "Support",
            Some("General help requests"),
            Some("#3b82f6"),
            Some("question"),
            0,
        ),
        (
            "Bug",
            Some("Defect reports"),
            Some("#ef4444"),
            Some("bug"),
            1,
        ),
        (
            "Feature request",
            Some("Enhancement ideas"),
            Some("#8b5cf6"),
            Some("lightbulb"),
            2,
        ),
    ];

    let rows: Vec<NewTicketCategory> = defaults
        .into_iter()
        .map(
            |(name, description, color, icon, display_order)| NewTicketCategory {
                name: name.to_string(),
                description: description.map(|s| s.to_string()),
                color: color.map(|s| s.to_string()),
                icon: icon.map(|s| s.to_string()),
                display_order,
                is_active: true,
                created_by,
            },
        )
        .collect();

    diesel::insert_into(ticket_categories::table)
        .values(&rows)
        .on_conflict((ticket_categories::workspace_id, ticket_categories::name))
        .do_nothing()
        .execute(conn)
}

/// Get the next display order value
pub fn get_next_display_order(conn: &mut DbConnection) -> QueryResult<i32> {
    let max_order: Option<i32> = ticket_categories::table
        .select(diesel::dsl::max(ticket_categories::display_order))
        .first(conn)?;

    Ok(max_order.unwrap_or(0) + 1)
}

// sync-pending-wire: needs sync aggregate wiring
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
pub fn get_visible_groups_for_category(
    conn: &mut DbConnection,
    category_id: i32,
) -> QueryResult<Vec<Group>> {
    category_group_visibility::table
        .filter(category_group_visibility::category_id.eq(category_id))
        .inner_join(groups::table)
        .select(groups::all_columns)
        .order(groups::name.asc())
        .load(conn)
}

/// Batched `get_visible_groups_for_category`: one join returning a
/// `category_id -> visible groups` map for many categories.
pub fn get_visible_groups_for_categories(
    conn: &mut DbConnection,
    category_ids: &[i32],
) -> QueryResult<std::collections::HashMap<i32, Vec<Group>>> {
    let rows: Vec<(i32, Group)> = category_group_visibility::table
        .filter(category_group_visibility::category_id.eq_any(category_ids))
        .inner_join(groups::table)
        .select((category_group_visibility::category_id, groups::all_columns))
        .order(groups::name.asc())
        .load(conn)?;
    let mut map: std::collections::HashMap<i32, Vec<Group>> = std::collections::HashMap::new();
    for (category_id, group) in rows {
        map.entry(category_id).or_default().push(group);
    }
    Ok(map)
}

// sync-pending-wire: needs sync aggregate wiring
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
            .filter(category_group_visibility::category_id.eq(category_id)),
    )
    .execute(conn)?;

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
    let user_group_ids: Vec<i32> =
        crate::repository::groups::get_group_ids_for_user(conn, user_uuid)?;

    // Get all active categories
    let all_categories = ticket_categories::table
        .filter(ticket_categories::is_active.eq(true))
        .order(ticket_categories::display_order.asc())
        .load::<TicketCategory>(conn)?;

    // Batch the per-category visibility into one query, then filter in memory:
    // a category with no visibility rows is public; otherwise the user must
    // share one of its allowed groups.
    let ids: Vec<i32> = all_categories.iter().map(|c| c.id).collect();
    let groups_map = get_visible_groups_for_categories(conn, &ids)?;

    Ok(all_categories
        .into_iter()
        .filter(|category| match groups_map.get(&category.id) {
            None => true,
            Some(groups) => groups.iter().any(|g| user_group_ids.contains(&g.id)),
        })
        .collect())
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
    let user_group_ids: Vec<i32> =
        crate::repository::groups::get_group_ids_for_user(conn, user_uuid)?;

    // Check if user is in any of the allowed groups
    Ok(user_group_ids
        .iter()
        .any(|id| category_group_ids.contains(id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn public_category_visible_to_any_user() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "alice", "user");
        let cat = TestFixtures::create_category(&mut conn, "Public");
        // No group restrictions → public
        assert!(can_user_see_category(&mut conn, &user.uuid, cat.id, false).unwrap());
    }

    #[test]
    fn restricted_category_visible_to_allowed_group() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "bob", "user");
        let group = TestFixtures::create_group(&mut conn, "Support");
        TestFixtures::add_user_to_group(&mut conn, user.uuid, group.id);
        let cat = TestFixtures::create_category(&mut conn, "VIP");
        TestFixtures::set_category_visibility(&mut conn, cat.id, &[group.id]);

        assert!(can_user_see_category(&mut conn, &user.uuid, cat.id, false).unwrap());
    }

    #[test]
    fn restricted_category_hidden_from_non_member() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "carol", "user");
        let group = TestFixtures::create_group(&mut conn, "VIP Group");
        // user is NOT added to the group
        let cat = TestFixtures::create_category(&mut conn, "VIP Only");
        TestFixtures::set_category_visibility(&mut conn, cat.id, &[group.id]);

        assert!(!can_user_see_category(&mut conn, &user.uuid, cat.id, false).unwrap());
    }

    #[test]
    fn get_categories_for_user_filters_by_batched_visibility() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "dave", "user");
        let group = TestFixtures::create_group(&mut conn, "Team");
        TestFixtures::add_user_to_group(&mut conn, user.uuid, group.id);

        let public = TestFixtures::create_category(&mut conn, "Public");
        let allowed = TestFixtures::create_category(&mut conn, "Allowed");
        TestFixtures::set_category_visibility(&mut conn, allowed.id, &[group.id]);
        let other_group = TestFixtures::create_group(&mut conn, "Other");
        let hidden = TestFixtures::create_category(&mut conn, "Hidden");
        TestFixtures::set_category_visibility(&mut conn, hidden.id, &[other_group.id]);

        let ids: Vec<i32> = get_categories_for_user(&mut conn, &user.uuid, false)
            .unwrap()
            .iter()
            .map(|c| c.id)
            .collect();
        assert!(ids.contains(&public.id), "public category visible");
        assert!(
            ids.contains(&allowed.id),
            "category shared with user's group visible"
        );
        assert!(
            !ids.contains(&hidden.id),
            "category for another group hidden"
        );
    }

    #[test]
    fn admin_sees_restricted_category() {
        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "admin", "admin");
        let group = TestFixtures::create_group(&mut conn, "Secret");
        let cat = TestFixtures::create_category(&mut conn, "Secret Cat");
        TestFixtures::set_category_visibility(&mut conn, cat.id, &[group.id]);
        // admin not in group but passes is_admin=true
        assert!(can_user_see_category(&mut conn, &admin.uuid, cat.id, true).unwrap());
    }

    #[test]
    fn seed_defaults_inserts_three_when_empty() {
        let mut conn = setup_test_connection();
        // Test fixture starts with no categories.
        let inserted = seed_defaults_if_empty(&mut conn, None).unwrap();
        assert_eq!(inserted, 3);

        let all: Vec<TicketCategory> = ticket_categories::table.load(&mut conn).unwrap();
        let names: Vec<&str> = all.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Support"));
        assert!(names.contains(&"Bug"));
        assert!(names.contains(&"Feature request"));
        // Display order is preserved.
        let mut sorted = all.clone();
        sorted.sort_by_key(|c| c.display_order);
        assert_eq!(sorted[0].name, "Support");
        assert_eq!(sorted[1].name, "Bug");
        assert_eq!(sorted[2].name, "Feature request");
    }

    #[test]
    fn seed_defaults_is_noop_when_categories_exist() {
        let mut conn = setup_test_connection();
        // Pre-existing category: any further seed call should be a no-op.
        TestFixtures::create_category(&mut conn, "Pre-existing");
        let inserted = seed_defaults_if_empty(&mut conn, None).unwrap();
        assert_eq!(inserted, 0);

        let count: i64 = ticket_categories::table
            .select(diesel::dsl::count_star())
            .first(&mut conn)
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn seed_defaults_records_creator_when_provided() {
        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "admin", "admin");
        seed_defaults_if_empty(&mut conn, Some(admin.uuid)).unwrap();
        let all: Vec<TicketCategory> = ticket_categories::table.load(&mut conn).unwrap();
        for cat in all {
            assert_eq!(cat.created_by, Some(admin.uuid));
        }
    }

    #[test]
    fn get_categories_returns_public_and_accessible() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "dave", "user");
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
