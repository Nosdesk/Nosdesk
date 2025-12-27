use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

// ============================================================================
// Group CRUD Operations
// ============================================================================

/// Get all groups
pub fn get_all_groups(conn: &mut DbConnection) -> QueryResult<Vec<Group>> {
    groups::table
        .order(groups::name.asc())
        .load(conn)
}

/// Get all groups with member counts
pub fn get_groups_with_member_counts(conn: &mut DbConnection) -> Result<Vec<GroupWithMemberCount>, Error> {
    let all_groups = groups::table
        .order(groups::name.asc())
        .load::<Group>(conn)?;

    let mut groups_with_count = Vec::new();

    for group in all_groups {
        let count = user_groups::table
            .filter(user_groups::group_id.eq(group.id))
            .count()
            .get_result::<i64>(conn)?;

        groups_with_count.push(GroupWithMemberCount {
            group,
            member_count: count,
        });
    }

    Ok(groups_with_count)
}

/// Get a group by ID
pub fn get_group_by_id(conn: &mut DbConnection, group_id: i32) -> QueryResult<Group> {
    groups::table.find(group_id).first(conn)
}

/// Get a group by UUID
pub fn get_group_by_uuid(conn: &mut DbConnection, group_uuid: &Uuid) -> QueryResult<Group> {
    groups::table
        .filter(groups::uuid.eq(group_uuid))
        .first(conn)
}

/// Get a group with its members
pub fn get_group_with_members(conn: &mut DbConnection, group_id: i32) -> Result<GroupWithMembers, Error> {
    let group = groups::table.find(group_id).first::<Group>(conn)?;

    let member_uuids: Vec<Uuid> = user_groups::table
        .filter(user_groups::group_id.eq(group_id))
        .select(user_groups::user_uuid)
        .load::<Uuid>(conn)?;

    let members: Vec<UserInfoWithAvatar> = member_uuids
        .iter()
        .filter_map(|uuid| {
            crate::repository::get_user_by_uuid(uuid, conn)
                .ok()
                .map(UserInfoWithAvatar::from)
        })
        .collect();

    Ok(GroupWithMembers { group, members })
}

/// Create a new group
pub fn create_group(conn: &mut DbConnection, new_group: NewGroup) -> QueryResult<Group> {
    diesel::insert_into(groups::table)
        .values(&new_group)
        .get_result(conn)
}

/// Update a group
pub fn update_group(conn: &mut DbConnection, group_id: i32, mut group_update: GroupUpdate) -> QueryResult<Group> {
    // Set updated_at to current time if not provided
    if group_update.updated_at.is_none() {
        group_update.updated_at = Some(chrono::Utc::now().naive_utc());
    }

    diesel::update(groups::table.find(group_id))
        .set(&group_update)
        .get_result(conn)
}

/// Delete a group (cascades to user_groups and category_group_visibility)
pub fn delete_group(conn: &mut DbConnection, group_id: i32) -> QueryResult<usize> {
    diesel::delete(groups::table.find(group_id)).execute(conn)
}

// ============================================================================
// User-Group Membership Operations
// ============================================================================

/// Get all users in a group
pub fn get_users_in_group(conn: &mut DbConnection, group_id: i32) -> QueryResult<Vec<User>> {
    user_groups::table
        .filter(user_groups::group_id.eq(group_id))
        .inner_join(users::table.on(users::uuid.eq(user_groups::user_uuid)))
        .select(users::all_columns)
        .load(conn)
}

/// Get all groups for a user
pub fn get_groups_for_user(conn: &mut DbConnection, user_uuid: &Uuid) -> QueryResult<Vec<Group>> {
    user_groups::table
        .filter(user_groups::user_uuid.eq(user_uuid))
        .inner_join(groups::table)
        .select(groups::all_columns)
        .order(groups::name.asc())
        .load(conn)
}

/// Add a user to a group
pub fn add_user_to_group(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    group_id: i32,
    created_by: Option<Uuid>,
) -> QueryResult<UserGroup> {
    // Check if already exists
    let existing = user_groups::table
        .filter(user_groups::user_uuid.eq(user_uuid))
        .filter(user_groups::group_id.eq(group_id))
        .first::<UserGroup>(conn);

    if let Ok(membership) = existing {
        return Ok(membership);
    }

    let new_membership = NewUserGroup {
        user_uuid,
        group_id,
        created_by,
    };

    diesel::insert_into(user_groups::table)
        .values(&new_membership)
        .get_result(conn)
}

/// Remove a user from a group
pub fn remove_user_from_group(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    group_id: i32,
) -> QueryResult<usize> {
    diesel::delete(
        user_groups::table
            .filter(user_groups::user_uuid.eq(user_uuid))
            .filter(user_groups::group_id.eq(group_id))
    ).execute(conn)
}

/// Set all members of a group (replaces existing members)
pub fn set_group_members(
    conn: &mut DbConnection,
    group_id: i32,
    member_uuids: Vec<Uuid>,
    created_by: Option<Uuid>,
) -> QueryResult<Vec<UserGroup>> {
    // Delete all existing members
    diesel::delete(
        user_groups::table.filter(user_groups::group_id.eq(group_id))
    ).execute(conn)?;

    // Add new members
    let new_memberships: Vec<NewUserGroup> = member_uuids
        .iter()
        .map(|uuid| NewUserGroup {
            user_uuid: *uuid,
            group_id,
            created_by,
        })
        .collect();

    if new_memberships.is_empty() {
        return Ok(Vec::new());
    }

    diesel::insert_into(user_groups::table)
        .values(&new_memberships)
        .get_results(conn)
}

/// Set all groups for a user (replaces existing group memberships)
pub fn set_user_groups(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    group_ids: Vec<i32>,
    created_by: Option<Uuid>,
) -> QueryResult<Vec<UserGroup>> {
    // Delete all existing memberships for this user
    diesel::delete(
        user_groups::table.filter(user_groups::user_uuid.eq(user_uuid))
    ).execute(conn)?;

    // Add new memberships
    let new_memberships: Vec<NewUserGroup> = group_ids
        .iter()
        .map(|group_id| NewUserGroup {
            user_uuid,
            group_id: *group_id,
            created_by,
        })
        .collect();

    if new_memberships.is_empty() {
        return Ok(Vec::new());
    }

    diesel::insert_into(user_groups::table)
        .values(&new_memberships)
        .get_results(conn)
}

/// Check if a user is in a specific group
pub fn is_user_in_group(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    group_id: i32,
) -> QueryResult<bool> {
    let count = user_groups::table
        .filter(user_groups::user_uuid.eq(user_uuid))
        .filter(user_groups::group_id.eq(group_id))
        .count()
        .get_result::<i64>(conn)?;

    Ok(count > 0)
}

/// Get group IDs for a user
pub fn get_group_ids_for_user(conn: &mut DbConnection, user_uuid: &Uuid) -> QueryResult<Vec<i32>> {
    user_groups::table
        .filter(user_groups::user_uuid.eq(user_uuid))
        .select(user_groups::group_id)
        .load(conn)
}
