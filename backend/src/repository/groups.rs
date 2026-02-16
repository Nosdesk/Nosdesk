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

/// Get all groups with member and device counts
pub fn get_groups_with_member_counts(conn: &mut DbConnection) -> Result<Vec<GroupWithMemberCount>, Error> {
    let all_groups = groups::table
        .order(groups::name.asc())
        .load::<Group>(conn)?;

    let mut groups_with_count = Vec::new();

    for group in all_groups {
        // Direct members
        let direct_uuids: Vec<Uuid> = user_groups::table
            .filter(user_groups::group_id.eq(group.id))
            .select(user_groups::user_uuid)
            .load(conn)?;

        // Members from included child groups
        let child_ids: Vec<i32> = group_includes::table
            .filter(group_includes::parent_group_id.eq(group.id))
            .select(group_includes::child_group_id)
            .load(conn)?;

        let member_count = if child_ids.is_empty() {
            direct_uuids.len() as i64
        } else {
            let child_uuids: Vec<Uuid> = user_groups::table
                .filter(user_groups::group_id.eq_any(&child_ids))
                .select(user_groups::user_uuid)
                .load(conn)?;

            let mut all_uuids: std::collections::HashSet<Uuid> = direct_uuids.into_iter().collect();
            all_uuids.extend(child_uuids);
            all_uuids.len() as i64
        };

        let device_count = device_groups::table
            .filter(device_groups::group_id.eq(group.id))
            .count()
            .get_result::<i64>(conn)?;

        let included_group_count = child_ids.len() as i64;

        groups_with_count.push(GroupWithMemberCount {
            group,
            member_count,
            device_count,
            included_group_count,
        });
    }

    Ok(groups_with_count)
}

/// Get a group by ID
pub fn get_group_by_id(conn: &mut DbConnection, group_id: i32) -> QueryResult<Group> {
    groups::table.find(group_id).first(conn)
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

/// Build a GroupSummary for a group with its member count and members
fn build_group_summary(conn: &mut DbConnection, group: &Group) -> Result<GroupSummary, Error> {
    let member_uuids: Vec<Uuid> = user_groups::table
        .filter(user_groups::group_id.eq(group.id))
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

    let member_count = members.len() as i64;

    Ok(GroupSummary {
        id: group.id,
        uuid: group.uuid,
        name: group.name.clone(),
        color: group.color.clone(),
        external_source: group.external_source.clone(),
        member_count,
        members,
    })
}

/// Get a group with its members and devices (for detail view)
pub fn get_group_details(conn: &mut DbConnection, group_uuid: &Uuid) -> Result<GroupDetails, Error> {
    let group = groups::table
        .filter(groups::uuid.eq(group_uuid))
        .first::<Group>(conn)?;

    let member_uuids: Vec<Uuid> = user_groups::table
        .filter(user_groups::group_id.eq(group.id))
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

    let devices: Vec<Device> = get_devices_in_group(conn, group.id)?;

    // Load included groups (children of this group)
    let child_groups: Vec<Group> = group_includes::table
        .filter(group_includes::parent_group_id.eq(group.id))
        .inner_join(groups::table.on(groups::id.eq(group_includes::child_group_id)))
        .select(groups::all_columns)
        .order(groups::name.asc())
        .load(conn)?;

    let included_groups: Vec<GroupSummary> = child_groups
        .iter()
        .filter_map(|g| build_group_summary(conn, g).ok())
        .collect();

    // Load parent groups that include this group
    let parent_groups: Vec<Group> = group_includes::table
        .filter(group_includes::child_group_id.eq(group.id))
        .inner_join(groups::table.on(groups::id.eq(group_includes::parent_group_id)))
        .select(groups::all_columns)
        .order(groups::name.asc())
        .load(conn)?;

    let included_in: Vec<GroupSummary> = parent_groups
        .iter()
        .filter_map(|g| build_group_summary(conn, g).ok())
        .collect();

    Ok(GroupDetails { group, members, devices, included_groups, included_in })
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

/// Unmanage a group (clear external source fields to make it locally managed)
pub fn unmanage_group(conn: &mut DbConnection, group_id: i32) -> QueryResult<Group> {
    diesel::update(groups::table.find(group_id))
        .set((
            groups::external_source.eq::<Option<String>>(None),
            groups::external_id.eq::<Option<String>>(None),
            groups::last_synced_at.eq::<Option<chrono::NaiveDateTime>>(None),
        ))
        .get_result(conn)
}

// ============================================================================
// User-Group Membership Operations
// ============================================================================

/// Get all users in a group (including members from included child groups)
pub fn get_users_in_group(conn: &mut DbConnection, group_id: i32) -> QueryResult<Vec<User>> {
    // Direct members
    let mut members: Vec<User> = user_groups::table
        .filter(user_groups::group_id.eq(group_id))
        .inner_join(users::table.on(users::uuid.eq(user_groups::user_uuid)))
        .select(users::all_columns)
        .load(conn)?;

    // Get child group IDs
    let child_ids: Vec<i32> = group_includes::table
        .filter(group_includes::parent_group_id.eq(group_id))
        .select(group_includes::child_group_id)
        .load(conn)?;

    if !child_ids.is_empty() {
        // Get members from child groups
        let child_members: Vec<User> = user_groups::table
            .filter(user_groups::group_id.eq_any(&child_ids))
            .inner_join(users::table.on(users::uuid.eq(user_groups::user_uuid)))
            .select(users::all_columns)
            .load(conn)?;

        // Deduplicate by UUID
        let existing_uuids: std::collections::HashSet<Uuid> = members.iter().map(|u| u.uuid).collect();
        for user in child_members {
            if !existing_uuids.contains(&user.uuid) {
                members.push(user);
            }
        }
    }

    Ok(members)
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

/// Get group IDs for a user (including composite parent groups)
pub fn get_group_ids_for_user(conn: &mut DbConnection, user_uuid: &Uuid) -> QueryResult<Vec<i32>> {
    let direct_ids: Vec<i32> = user_groups::table
        .filter(user_groups::user_uuid.eq(user_uuid))
        .select(user_groups::group_id)
        .load(conn)?;

    if direct_ids.is_empty() {
        return Ok(direct_ids);
    }

    // Find parent groups that include any of the user's direct groups
    let parent_ids: Vec<i32> = group_includes::table
        .filter(group_includes::child_group_id.eq_any(&direct_ids))
        .select(group_includes::parent_group_id)
        .load(conn)?;

    // Merge and deduplicate
    let mut all_ids = direct_ids;
    for pid in parent_ids {
        if !all_ids.contains(&pid) {
            all_ids.push(pid);
        }
    }

    Ok(all_ids)
}

// ============================================================================
// External Group Sync Operations (Microsoft Graph, etc.)
// ============================================================================

/// Get a group by its external ID
#[allow(dead_code)]
pub fn get_group_by_external_id(conn: &mut DbConnection, external_id: &str) -> QueryResult<Group> {
    groups::table
        .filter(groups::external_id.eq(external_id))
        .first(conn)
}

/// Get all groups from a specific external source
#[allow(dead_code)]
pub fn get_groups_by_external_source(conn: &mut DbConnection, external_source: &str) -> QueryResult<Vec<Group>> {
    groups::table
        .filter(groups::external_source.eq(external_source))
        .order(groups::name.asc())
        .load(conn)
}

/// Get all external IDs for groups from a specific source
#[allow(dead_code)]
pub fn get_external_ids_by_source(conn: &mut DbConnection, external_source: &str) -> QueryResult<Vec<String>> {
    groups::table
        .filter(groups::external_source.eq(external_source))
        .filter(groups::external_id.is_not_null())
        .select(groups::external_id)
        .load::<Option<String>>(conn)
        .map(|ids| ids.into_iter().flatten().collect())
}

/// Create a group from external source data
#[allow(dead_code)]
pub fn create_external_group(conn: &mut DbConnection, new_group: NewExternalGroup) -> QueryResult<Group> {
    diesel::insert_into(groups::table)
        .values(&new_group)
        .get_result(conn)
}

/// Update a group from external source data
#[allow(dead_code)]
pub fn update_external_group(
    conn: &mut DbConnection,
    group_id: i32,
    mut group_update: ExternalGroupUpdate,
) -> QueryResult<Group> {
    if group_update.updated_at.is_none() {
        group_update.updated_at = Some(chrono::Utc::now().naive_utc());
    }
    if group_update.last_synced_at.is_none() {
        group_update.last_synced_at = Some(chrono::Utc::now().naive_utc());
    }

    diesel::update(groups::table.find(group_id))
        .set(&group_update)
        .get_result(conn)
}

/// Upsert a group from external source - returns (group, was_created)
pub fn upsert_external_group(
    conn: &mut DbConnection,
    external_id: &str,
    external_source: &str,
    name: &str,
    description: Option<&str>,
    group_type: Option<&str>,
    mail_enabled: bool,
    security_enabled: bool,
) -> QueryResult<(Group, bool)> {
    // Try to find existing group by external_id
    let existing = groups::table
        .filter(groups::external_id.eq(external_id))
        .first::<Group>(conn);

    match existing {
        Ok(group) => {
            // Update existing group
            let update = ExternalGroupUpdate {
                name: Some(name.to_string()),
                description: description.map(String::from),
                group_type: group_type.map(String::from),
                mail_enabled: Some(mail_enabled),
                security_enabled: Some(security_enabled),
                last_synced_at: Some(chrono::Utc::now().naive_utc()),
                updated_at: Some(chrono::Utc::now().naive_utc()),
            };

            let updated = diesel::update(groups::table.find(group.id))
                .set(&update)
                .get_result(conn)?;

            Ok((updated, false))
        }
        Err(diesel::result::Error::NotFound) => {
            // Create new group
            let new_group = NewExternalGroup {
                name: name.to_string(),
                description: description.map(String::from),
                external_id: Some(external_id.to_string()),
                external_source: Some(external_source.to_string()),
                group_type: group_type.map(String::from),
                mail_enabled,
                security_enabled,
            };

            let created = diesel::insert_into(groups::table)
                .values(&new_group)
                .get_result(conn)?;

            Ok((created, true))
        }
        Err(e) => Err(e),
    }
}

/// Get member UUIDs for a group (simple list)
pub fn get_member_uuids_for_group(conn: &mut DbConnection, group_id: i32) -> QueryResult<Vec<Uuid>> {
    user_groups::table
        .filter(user_groups::group_id.eq(group_id))
        .select(user_groups::user_uuid)
        .load(conn)
}

/// Mark groups as stale (not seen in this sync) - useful for detecting deleted external groups
pub fn mark_groups_not_synced(
    conn: &mut DbConnection,
    external_source: &str,
    except_external_ids: &[&str],
) -> QueryResult<usize> {
    use diesel::dsl::now;

    // This updates sync_enabled to false for groups that are:
    // 1. From the specified external source
    // 2. NOT in the list of external IDs we just synced
    // This doesn't delete them - it just marks them so they can be cleaned up later if desired
    diesel::update(
        groups::table
            .filter(groups::external_source.eq(external_source))
            .filter(groups::external_id.is_not_null())
            .filter(diesel::dsl::not(groups::external_id.eq_any(except_external_ids)))
    )
    .set((
        groups::sync_enabled.eq(false),
        groups::updated_at.eq(now),
    ))
    .execute(conn)
}

// ============================================================================
// Group Includes (Composite Groups)
// ============================================================================

/// Get the child groups included in a parent group
pub fn get_included_groups(conn: &mut DbConnection, parent_group_id: i32) -> QueryResult<Vec<Group>> {
    group_includes::table
        .filter(group_includes::parent_group_id.eq(parent_group_id))
        .inner_join(groups::table.on(groups::id.eq(group_includes::child_group_id)))
        .select(groups::all_columns)
        .order(groups::name.asc())
        .load(conn)
}

/// Get the parent groups that include a given child group
pub fn get_parent_groups(conn: &mut DbConnection, child_group_id: i32) -> QueryResult<Vec<Group>> {
    group_includes::table
        .filter(group_includes::child_group_id.eq(child_group_id))
        .inner_join(groups::table.on(groups::id.eq(group_includes::parent_group_id)))
        .select(groups::all_columns)
        .order(groups::name.asc())
        .load(conn)
}

/// Add a group include relationship with validation
pub fn add_group_include(
    conn: &mut DbConnection,
    parent_id: i32,
    child_id: i32,
    created_by: Option<Uuid>,
) -> Result<GroupInclude, Error> {
    // Self-inclusion check (also enforced by DB CHECK constraint)
    if parent_id == child_id {
        return Err(Error::DatabaseError(
            diesel::result::DatabaseErrorKind::CheckViolation,
            Box::new("A group cannot include itself".to_string()),
        ));
    }

    // Parent must not be a managed (externally synced) group
    let parent = groups::table.find(parent_id).first::<Group>(conn)?;
    if parent.external_source.is_some() {
        return Err(Error::DatabaseError(
            diesel::result::DatabaseErrorKind::CheckViolation,
            Box::new("Externally managed groups cannot be composite parents".to_string()),
        ));
    }

    // Circular reference check: child must not already include parent
    let reverse_exists: i64 = group_includes::table
        .filter(group_includes::parent_group_id.eq(child_id))
        .filter(group_includes::child_group_id.eq(parent_id))
        .count()
        .get_result(conn)?;

    if reverse_exists > 0 {
        return Err(Error::DatabaseError(
            diesel::result::DatabaseErrorKind::CheckViolation,
            Box::new("Circular reference: child group already includes the parent".to_string()),
        ));
    }

    let new_include = NewGroupInclude {
        parent_group_id: parent_id,
        child_group_id: child_id,
        created_by,
    };

    diesel::insert_into(group_includes::table)
        .values(&new_include)
        .on_conflict((group_includes::parent_group_id, group_includes::child_group_id))
        .do_nothing()
        .execute(conn)?;

    group_includes::table
        .filter(group_includes::parent_group_id.eq(parent_id))
        .filter(group_includes::child_group_id.eq(child_id))
        .first(conn)
}

/// Remove a group include relationship
pub fn remove_group_include(
    conn: &mut DbConnection,
    parent_id: i32,
    child_id: i32,
) -> QueryResult<usize> {
    diesel::delete(
        group_includes::table
            .filter(group_includes::parent_group_id.eq(parent_id))
            .filter(group_includes::child_group_id.eq(child_id)),
    )
    .execute(conn)
}

/// Set all included groups for a parent (replaces existing includes)
pub fn set_group_includes(
    conn: &mut DbConnection,
    parent_id: i32,
    child_ids: Vec<i32>,
    created_by: Option<Uuid>,
) -> Result<Vec<GroupInclude>, Error> {
    // Parent must not be a managed group
    let parent = groups::table.find(parent_id).first::<Group>(conn)?;
    if parent.external_source.is_some() {
        return Err(Error::DatabaseError(
            diesel::result::DatabaseErrorKind::CheckViolation,
            Box::new("Externally managed groups cannot be composite parents".to_string()),
        ));
    }

    // Filter out self-inclusion
    let child_ids: Vec<i32> = child_ids.into_iter().filter(|&id| id != parent_id).collect();

    // Circular reference check: none of the children should already include parent
    if !child_ids.is_empty() {
        let circular_count: i64 = group_includes::table
            .filter(group_includes::parent_group_id.eq_any(&child_ids))
            .filter(group_includes::child_group_id.eq(parent_id))
            .count()
            .get_result(conn)?;

        if circular_count > 0 {
            return Err(Error::DatabaseError(
                diesel::result::DatabaseErrorKind::CheckViolation,
                Box::new("Circular reference: one or more child groups already include the parent".to_string()),
            ));
        }
    }

    // Delete existing includes
    diesel::delete(
        group_includes::table.filter(group_includes::parent_group_id.eq(parent_id)),
    )
    .execute(conn)?;

    if child_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Insert new includes
    let new_includes: Vec<NewGroupInclude> = child_ids
        .iter()
        .map(|&child_id| NewGroupInclude {
            parent_group_id: parent_id,
            child_group_id: child_id,
            created_by,
        })
        .collect();

    diesel::insert_into(group_includes::table)
        .values(&new_includes)
        .get_results(conn)
}

// ============================================================================
// Device-Group Membership Operations
// ============================================================================

/// Get all devices in a group
pub fn get_devices_in_group(conn: &mut DbConnection, group_id: i32) -> QueryResult<Vec<Device>> {
    device_groups::table
        .filter(device_groups::group_id.eq(group_id))
        .inner_join(devices::table.on(devices::id.eq(device_groups::device_id)))
        .select(devices::all_columns)
        .load(conn)
}

/// Get all groups for a device
pub fn get_groups_for_device(conn: &mut DbConnection, device_id: i32) -> QueryResult<Vec<Group>> {
    device_groups::table
        .filter(device_groups::device_id.eq(device_id))
        .inner_join(groups::table)
        .select(groups::all_columns)
        .order(groups::name.asc())
        .load(conn)
}

/// Add a device to a group
pub fn add_device_to_group(
    conn: &mut DbConnection,
    device_id: i32,
    group_id: i32,
    created_by: Option<Uuid>,
    external_source: Option<&str>,
) -> QueryResult<DeviceGroup> {
    // Check if already exists
    let existing = device_groups::table
        .filter(device_groups::device_id.eq(device_id))
        .filter(device_groups::group_id.eq(group_id))
        .first::<DeviceGroup>(conn);

    if let Ok(membership) = existing {
        return Ok(membership);
    }

    let new_membership = NewDeviceGroup {
        device_id,
        group_id,
        created_by,
        external_source: external_source.map(String::from),
    };

    diesel::insert_into(device_groups::table)
        .values(&new_membership)
        .get_result(conn)
}

/// Remove a device from a group
pub fn remove_device_from_group(
    conn: &mut DbConnection,
    device_id: i32,
    group_id: i32,
) -> QueryResult<usize> {
    diesel::delete(
        device_groups::table
            .filter(device_groups::device_id.eq(device_id))
            .filter(device_groups::group_id.eq(group_id))
    ).execute(conn)
}

/// Get device IDs for a group (simple list)
pub fn get_device_ids_for_group(conn: &mut DbConnection, group_id: i32) -> QueryResult<Vec<i32>> {
    device_groups::table
        .filter(device_groups::group_id.eq(group_id))
        .select(device_groups::device_id)
        .load(conn)
}

/// Get device IDs for a group that were synced from an external source
pub fn get_synced_device_ids_for_group(
    conn: &mut DbConnection,
    group_id: i32,
    external_source: &str,
) -> QueryResult<Vec<i32>> {
    device_groups::table
        .filter(device_groups::group_id.eq(group_id))
        .filter(device_groups::external_source.eq(external_source))
        .select(device_groups::device_id)
        .load(conn)
}

/// Set all devices of a group (replaces existing non-synced devices)
/// Note: This only removes manually-added devices, not externally synced ones
pub fn set_group_devices(
    conn: &mut DbConnection,
    group_id: i32,
    device_ids: Vec<i32>,
    created_by: Option<Uuid>,
) -> QueryResult<Vec<DeviceGroup>> {
    // Delete all existing devices that were NOT synced from an external source
    // This preserves Microsoft-synced device memberships
    diesel::delete(
        device_groups::table
            .filter(device_groups::group_id.eq(group_id))
            .filter(device_groups::external_source.is_null())
    ).execute(conn)?;

    // Add new devices (manually added, so no external_source)
    let new_memberships: Vec<NewDeviceGroup> = device_ids
        .iter()
        .map(|device_id| NewDeviceGroup {
            device_id: *device_id,
            group_id,
            created_by,
            external_source: None,
        })
        .collect();

    if new_memberships.is_empty() {
        return Ok(Vec::new());
    }

    // Use ON CONFLICT DO NOTHING to handle devices that are already in the group via sync
    diesel::insert_into(device_groups::table)
        .values(&new_memberships)
        .on_conflict((device_groups::device_id, device_groups::group_id))
        .do_nothing()
        .execute(conn)?;

    // Return the current state of device memberships
    device_groups::table
        .filter(device_groups::group_id.eq(group_id))
        .load(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use crate::models::UserRole;

    #[test]
    fn create_and_get_group() {
        let mut conn = setup_test_connection();
        let group = TestFixtures::create_group(&mut conn, "Engineering");

        let fetched = get_group_by_id(&mut conn, group.id).unwrap();
        assert_eq!(fetched.name, "Engineering");
    }

    #[test]
    fn add_and_get_users_in_group() {
        let mut conn = setup_test_connection();
        let group = TestFixtures::create_group(&mut conn, "Team");
        let u1 = TestFixtures::create_user(&mut conn, "alice", UserRole::User);
        let u2 = TestFixtures::create_user(&mut conn, "bob", UserRole::User);

        add_user_to_group(&mut conn, u1.uuid, group.id, None).unwrap();
        add_user_to_group(&mut conn, u2.uuid, group.id, None).unwrap();

        let members = get_users_in_group(&mut conn, group.id).unwrap();
        let uuids: Vec<Uuid> = members.iter().map(|u| u.uuid).collect();
        assert!(uuids.contains(&u1.uuid));
        assert!(uuids.contains(&u2.uuid));
    }

    #[test]
    fn add_user_to_group_is_idempotent() {
        let mut conn = setup_test_connection();
        let group = TestFixtures::create_group(&mut conn, "Idem");
        let user = TestFixtures::create_user(&mut conn, "idemuser", UserRole::User);

        let m1 = add_user_to_group(&mut conn, user.uuid, group.id, None).unwrap();
        let m2 = add_user_to_group(&mut conn, user.uuid, group.id, None).unwrap();
        assert_eq!(m1.user_uuid, m2.user_uuid);
        assert_eq!(m1.group_id, m2.group_id);

        let members = get_users_in_group(&mut conn, group.id).unwrap();
        assert_eq!(members.len(), 1);
    }

    #[test]
    fn remove_user_from_group_works() {
        let mut conn = setup_test_connection();
        let group = TestFixtures::create_group(&mut conn, "Remove");
        let user = TestFixtures::create_user(&mut conn, "rmuser", UserRole::User);

        add_user_to_group(&mut conn, user.uuid, group.id, None).unwrap();
        remove_user_from_group(&mut conn, &user.uuid, group.id).unwrap();

        assert!(get_users_in_group(&mut conn, group.id).unwrap().is_empty());
    }

    #[test]
    fn get_groups_for_user_works() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "multigroup", UserRole::User);
        let g1 = TestFixtures::create_group(&mut conn, "Alpha");
        let g2 = TestFixtures::create_group(&mut conn, "Beta");

        add_user_to_group(&mut conn, user.uuid, g1.id, None).unwrap();
        add_user_to_group(&mut conn, user.uuid, g2.id, None).unwrap();

        let groups = get_groups_for_user(&mut conn, &user.uuid).unwrap();
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert!(names.contains(&"Alpha"));
        assert!(names.contains(&"Beta"));
    }

    #[test]
    fn get_group_ids_for_user_works() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "iduser", UserRole::User);
        let g1 = TestFixtures::create_group(&mut conn, "G1");
        let g2 = TestFixtures::create_group(&mut conn, "G2");

        add_user_to_group(&mut conn, user.uuid, g1.id, None).unwrap();
        add_user_to_group(&mut conn, user.uuid, g2.id, None).unwrap();

        let ids = get_group_ids_for_user(&mut conn, &user.uuid).unwrap();
        assert!(ids.contains(&g1.id));
        assert!(ids.contains(&g2.id));
    }

    #[test]
    fn set_group_members_replaces_all() {
        let mut conn = setup_test_connection();
        let group = TestFixtures::create_group(&mut conn, "Replace");
        let u1 = TestFixtures::create_user(&mut conn, "old", UserRole::User);
        let u2 = TestFixtures::create_user(&mut conn, "new1", UserRole::User);
        let u3 = TestFixtures::create_user(&mut conn, "new2", UserRole::User);

        add_user_to_group(&mut conn, u1.uuid, group.id, None).unwrap();

        set_group_members(&mut conn, group.id, vec![u2.uuid, u3.uuid], None).unwrap();

        let members = get_users_in_group(&mut conn, group.id).unwrap();
        let uuids: Vec<Uuid> = members.iter().map(|u| u.uuid).collect();
        assert!(!uuids.contains(&u1.uuid));
        assert!(uuids.contains(&u2.uuid));
        assert!(uuids.contains(&u3.uuid));
    }

    #[test]
    fn delete_group_removes_it() {
        let mut conn = setup_test_connection();
        let group = TestFixtures::create_group(&mut conn, "Doomed");

        delete_group(&mut conn, group.id).unwrap();
        assert!(get_group_by_id(&mut conn, group.id).is_err());
    }

    #[test]
    fn groups_with_member_counts() {
        let mut conn = setup_test_connection();
        let group = TestFixtures::create_group(&mut conn, "Counted");
        let u1 = TestFixtures::create_user(&mut conn, "c1", UserRole::User);
        let u2 = TestFixtures::create_user(&mut conn, "c2", UserRole::User);
        add_user_to_group(&mut conn, u1.uuid, group.id, None).unwrap();
        add_user_to_group(&mut conn, u2.uuid, group.id, None).unwrap();

        let all = get_groups_with_member_counts(&mut conn).unwrap();
        let found = all.iter().find(|g| g.group.id == group.id).unwrap();
        assert_eq!(found.member_count, 2);
    }
}
