use diesel::prelude::*;
use diesel::result::Error;
use diesel::Connection;
use diesel::QueryResult;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups as sync_groups;

// ============================================================================
// Group CRUD Operations
// ============================================================================

/// Get all groups with member and device counts (batch-loaded, no N+1)
pub fn get_groups_with_member_counts(
    conn: &mut DbConnection,
) -> Result<Vec<GroupWithMemberCount>, Error> {
    let all_groups = groups::table
        .order(groups::name.asc())
        .load::<Group>(conn)?;

    // Batch: all direct memberships (group_id → user_uuid)
    let all_memberships: Vec<(i32, Uuid)> = user_groups::table
        .select((user_groups::group_id, user_groups::user_uuid))
        .load(conn)?;

    // Batch: all group-include relationships (parent → child)
    let all_includes: Vec<(i32, i32)> = group_includes::table
        .select((
            group_includes::parent_group_id,
            group_includes::child_group_id,
        ))
        .load(conn)?;

    // Batch: all device counts per group
    let device_counts: Vec<(i32, i64)> = asset_groups::table
        .group_by(asset_groups::group_id)
        .select((asset_groups::group_id, diesel::dsl::count_star()))
        .load(conn)?;

    // Build lookup maps
    let mut members_by_group: std::collections::HashMap<i32, Vec<Uuid>> =
        std::collections::HashMap::new();
    for (group_id, user_uuid) in &all_memberships {
        members_by_group
            .entry(*group_id)
            .or_default()
            .push(*user_uuid);
    }

    let mut children_by_parent: std::collections::HashMap<i32, Vec<i32>> =
        std::collections::HashMap::new();
    for (parent_id, child_id) in &all_includes {
        children_by_parent
            .entry(*parent_id)
            .or_default()
            .push(*child_id);
    }

    let device_count_map: std::collections::HashMap<i32, i64> = device_counts.into_iter().collect();

    // Compute counts in memory
    let groups_with_count = all_groups
        .into_iter()
        .map(|group| {
            let direct_uuids = members_by_group.get(&group.id).cloned().unwrap_or_default();
            let child_ids = children_by_parent
                .get(&group.id)
                .cloned()
                .unwrap_or_default();

            let member_count = if child_ids.is_empty() {
                direct_uuids.len() as i64
            } else {
                let mut all_uuids: std::collections::HashSet<Uuid> =
                    direct_uuids.into_iter().collect();
                for child_id in &child_ids {
                    if let Some(child_members) = members_by_group.get(child_id) {
                        all_uuids.extend(child_members);
                    }
                }
                all_uuids.len() as i64
            };

            let device_count = device_count_map.get(&group.id).copied().unwrap_or(0);
            let included_group_count = child_ids.len() as i64;

            GroupWithMemberCount {
                group,
                member_count,
                device_count,
                included_group_count,
            }
        })
        .collect();

    Ok(groups_with_count)
}

/// Get a group by ID
pub fn get_group_by_id(conn: &mut DbConnection, group_id: i32) -> QueryResult<Group> {
    groups::table.find(group_id).first(conn)
}

/// Get a group with its members (batch-loaded)
pub fn get_group_with_members(
    conn: &mut DbConnection,
    group_id: i32,
) -> Result<GroupWithMembers, Error> {
    let group = groups::table.find(group_id).first::<Group>(conn)?;

    let member_uuids: Vec<Uuid> = user_groups::table
        .filter(user_groups::group_id.eq(group_id))
        .select(user_groups::user_uuid)
        .load::<Uuid>(conn)?;

    let users = crate::repository::users::get_users_by_uuids(&member_uuids, conn)?;
    let members: Vec<UserInfoWithAvatar> =
        users.into_iter().map(UserInfoWithAvatar::from).collect();

    Ok(GroupWithMembers { group, members })
}

/// Build a GroupSummary for a group with its member count and members (batch-loaded)
fn build_group_summary(conn: &mut DbConnection, group: &Group) -> Result<GroupSummary, Error> {
    let member_uuids: Vec<Uuid> = user_groups::table
        .filter(user_groups::group_id.eq(group.id))
        .select(user_groups::user_uuid)
        .load::<Uuid>(conn)?;

    let users = crate::repository::users::get_users_by_uuids(&member_uuids, conn)?;
    let members: Vec<UserInfoWithAvatar> =
        users.into_iter().map(UserInfoWithAvatar::from).collect();
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
pub fn get_group_details(
    conn: &mut DbConnection,
    group_uuid: &Uuid,
) -> Result<GroupDetails, Error> {
    let group = groups::table
        .filter(groups::uuid.eq(group_uuid))
        .first::<Group>(conn)?;

    let member_uuids: Vec<Uuid> = user_groups::table
        .filter(user_groups::group_id.eq(group.id))
        .select(user_groups::user_uuid)
        .load::<Uuid>(conn)?;

    let users = crate::repository::users::get_users_by_uuids(&member_uuids, conn)?;
    let members: Vec<UserInfoWithAvatar> =
        users.into_iter().map(UserInfoWithAvatar::from).collect();

    let devices: Vec<Asset> = get_devices_in_group(conn, group.id)?;

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

    Ok(GroupDetails {
        group,
        members,
        devices,
        included_groups,
        included_in,
    })
}

/// Create a new group
pub fn create_group(conn: &mut DbConnection, new_group: NewGroup) -> QueryResult<Group> {
    conn.transaction(|conn| {
        let group: Group = diesel::insert_into(groups::table)
            .values(&new_group)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::GroupMembership,
                aggregate_id: group.id.to_string(),
                op: SyncOp::Insert,
                event_type: "group.created",
                data: json!({
                    "id": group.id,
                    "uuid": group.uuid,
                    "name": group.name,
                    "description": group.description,
                    "color": group.color,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(group)
    })
}

/// Update a group
pub fn update_group(
    conn: &mut DbConnection,
    group_id: i32,
    mut group_update: GroupUpdate,
) -> QueryResult<Group> {
    // Set updated_at to current time if not provided
    if group_update.updated_at.is_none() {
        group_update.updated_at = Some(chrono::Utc::now().naive_utc());
    }

    conn.transaction(|conn| {
        let group: Group = diesel::update(groups::table.find(group_id))
            .set(&group_update)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::GroupMembership,
                aggregate_id: group.id.to_string(),
                op: SyncOp::Update,
                event_type: "group.updated",
                data: json!({
                    "id": group.id,
                    "uuid": group.uuid,
                    "name": group.name,
                    "description": group.description,
                    "color": group.color,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(group)
    })
}

/// Delete a group (cascades to user_groups and category_group_visibility)
pub fn delete_group(conn: &mut DbConnection, group_id: i32) -> QueryResult<usize> {
    conn.transaction(|conn| {
        let result = diesel::delete(groups::table.find(group_id)).execute(conn)?;
        if result > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::GroupMembership,
                    aggregate_id: group_id.to_string(),
                    op: SyncOp::Delete,
                    event_type: "group.deleted",
                    data: json!({ "id": group_id }),
                    groups: sync_groups::workspace(),
                    causation_id: None,
                },
            )?;
        }
        Ok(result)
    })
}

/// Unmanage a group (clear external source fields to make it locally managed)
pub fn unmanage_group(conn: &mut DbConnection, group_id: i32) -> QueryResult<Group> {
    conn.transaction(|conn| {
        let group: Group = diesel::update(groups::table.find(group_id))
            .set((
                groups::external_source.eq::<Option<String>>(None),
                groups::external_id.eq::<Option<String>>(None),
                groups::last_synced_at.eq::<Option<chrono::NaiveDateTime>>(None),
            ))
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::GroupMembership,
                aggregate_id: group.id.to_string(),
                op: SyncOp::Update,
                event_type: "group.unmanaged",
                data: json!({
                    "id": group.id,
                    "uuid": group.uuid,
                    "name": group.name,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(group)
    })
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
        let existing_uuids: std::collections::HashSet<Uuid> =
            members.iter().map(|u| u.uuid).collect();
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
    conn.transaction(|conn| {
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

        let membership: UserGroup = diesel::insert_into(user_groups::table)
            .values(&new_membership)
            .get_result(conn)?;

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::GroupMembership,
                aggregate_id: format!("{}:{}", group_id, user_uuid),
                op: SyncOp::Insert,
                event_type: "group_membership.user_added",
                data: json!({
                    "group_id": group_id,
                    "user_uuid": user_uuid,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(membership)
    })
}

/// Remove a user from a group
pub fn remove_user_from_group(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    group_id: i32,
) -> QueryResult<usize> {
    conn.transaction(|conn| {
        let result = diesel::delete(
            user_groups::table
                .filter(user_groups::user_uuid.eq(user_uuid))
                .filter(user_groups::group_id.eq(group_id)),
        )
        .execute(conn)?;
        if result > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::GroupMembership,
                    aggregate_id: format!("{}:{}", group_id, user_uuid),
                    op: SyncOp::Delete,
                    event_type: "group_membership.user_removed",
                    data: json!({
                        "group_id": group_id,
                        "user_uuid": user_uuid,
                    }),
                    groups: sync_groups::workspace(),
                    causation_id: None,
                },
            )?;
        }
        Ok(result)
    })
}

/// Set all members of a group (replaces existing members)
pub fn set_group_members(
    conn: &mut DbConnection,
    group_id: i32,
    member_uuids: Vec<Uuid>,
    created_by: Option<Uuid>,
) -> QueryResult<Vec<UserGroup>> {
    conn.transaction(|conn| {
        // Delete all existing members
        diesel::delete(user_groups::table.filter(user_groups::group_id.eq(group_id)))
            .execute(conn)?;

        // Add new members
        let new_memberships: Vec<NewUserGroup> = member_uuids
            .iter()
            .map(|uuid| NewUserGroup {
                user_uuid: *uuid,
                group_id,
                created_by,
            })
            .collect();

        let inserted: Vec<UserGroup> = if new_memberships.is_empty() {
            Vec::new()
        } else {
            diesel::insert_into(user_groups::table)
                .values(&new_memberships)
                .get_results(conn)?
        };

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::GroupMembership,
                aggregate_id: group_id.to_string(),
                op: SyncOp::Update,
                event_type: "group_membership.members_set",
                data: json!({
                    "group_id": group_id,
                    "user_uuids": member_uuids,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(inserted)
    })
}

/// Set all groups for a user (replaces existing group memberships)
pub fn set_user_groups(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    group_ids: Vec<i32>,
    created_by: Option<Uuid>,
) -> QueryResult<Vec<UserGroup>> {
    conn.transaction(|conn| {
        // Delete all existing memberships for this user
        diesel::delete(user_groups::table.filter(user_groups::user_uuid.eq(user_uuid)))
            .execute(conn)?;

        // Add new memberships
        let new_memberships: Vec<NewUserGroup> = group_ids
            .iter()
            .map(|group_id| NewUserGroup {
                user_uuid,
                group_id: *group_id,
                created_by,
            })
            .collect();

        let inserted: Vec<UserGroup> = if new_memberships.is_empty() {
            Vec::new()
        } else {
            diesel::insert_into(user_groups::table)
                .values(&new_memberships)
                .get_results(conn)?
        };

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::GroupMembership,
                aggregate_id: user_uuid.to_string(),
                op: SyncOp::Update,
                event_type: "group_membership.user_groups_set",
                data: json!({
                    "user_uuid": user_uuid,
                    "group_ids": group_ids,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(inserted)
    })
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
    conn.transaction(|conn| {
        // Try to find existing group by external_id
        let existing = groups::table
            .filter(groups::external_id.eq(external_id))
            .first::<Group>(conn);

        let (group, was_created) = match existing {
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

                let updated: Group = diesel::update(groups::table.find(group.id))
                    .set(&update)
                    .get_result(conn)?;

                (updated, false)
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

                let created: Group = diesel::insert_into(groups::table)
                    .values(&new_group)
                    .get_result(conn)?;

                (created, true)
            }
            Err(e) => return Err(e),
        };

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::GroupMembership,
                aggregate_id: group.id.to_string(),
                op: SyncOp::Update,
                event_type: "group.upserted_external",
                data: json!({
                    "id": group.id,
                    "uuid": group.uuid,
                    "name": group.name,
                    "external_id": external_id,
                    "external_source": external_source,
                    "was_created": was_created,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok((group, was_created))
    })
}

/// Get member UUIDs for a group (simple list)
pub fn get_member_uuids_for_group(
    conn: &mut DbConnection,
    group_id: i32,
) -> QueryResult<Vec<Uuid>> {
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

    conn.transaction(|conn| {
        // This updates sync_enabled to false for groups that are:
        // 1. From the specified external source
        // 2. NOT in the list of external IDs we just synced
        // This doesn't delete them - it just marks them so they can be cleaned up later if desired
        let result = diesel::update(
            groups::table
                .filter(groups::external_source.eq(external_source))
                .filter(groups::external_id.is_not_null())
                .filter(diesel::dsl::not(
                    groups::external_id.eq_any(except_external_ids),
                )),
        )
        .set((groups::sync_enabled.eq(false), groups::updated_at.eq(now)))
        .execute(conn)?;

        if result > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::GroupMembership,
                    aggregate_id: external_source.to_string(),
                    op: SyncOp::Update,
                    event_type: "group.marked_not_synced",
                    data: json!({
                        "external_source": external_source,
                        "kept_external_ids": except_external_ids,
                        "affected_count": result,
                    }),
                    groups: sync_groups::workspace(),
                    causation_id: None,
                },
            )?;
        }
        Ok(result)
    })
}

// ============================================================================
// Group Includes (Composite Groups)
// ============================================================================

/// Get the child groups included in a parent group
pub fn get_included_groups(
    conn: &mut DbConnection,
    parent_group_id: i32,
) -> QueryResult<Vec<Group>> {
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
    conn.transaction(|conn| {
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
            .on_conflict((
                group_includes::parent_group_id,
                group_includes::child_group_id,
            ))
            .do_nothing()
            .execute(conn)?;

        let include: GroupInclude = group_includes::table
            .filter(group_includes::parent_group_id.eq(parent_id))
            .filter(group_includes::child_group_id.eq(child_id))
            .first(conn)?;

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::GroupMembership,
                aggregate_id: format!("{}:{}", parent_id, child_id),
                op: SyncOp::Insert,
                event_type: "group_membership.include_added",
                data: json!({
                    "parent_group_id": parent_id,
                    "child_group_id": child_id,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(include)
    })
}

/// Remove a group include relationship
pub fn remove_group_include(
    conn: &mut DbConnection,
    parent_id: i32,
    child_id: i32,
) -> QueryResult<usize> {
    conn.transaction(|conn| {
        let result = diesel::delete(
            group_includes::table
                .filter(group_includes::parent_group_id.eq(parent_id))
                .filter(group_includes::child_group_id.eq(child_id)),
        )
        .execute(conn)?;
        if result > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::GroupMembership,
                    aggregate_id: format!("{}:{}", parent_id, child_id),
                    op: SyncOp::Delete,
                    event_type: "group_membership.include_removed",
                    data: json!({
                        "parent_group_id": parent_id,
                        "child_group_id": child_id,
                    }),
                    groups: sync_groups::workspace(),
                    causation_id: None,
                },
            )?;
        }
        Ok(result)
    })
}

/// Set all included groups for a parent (replaces existing includes)
pub fn set_group_includes(
    conn: &mut DbConnection,
    parent_id: i32,
    child_ids: Vec<i32>,
    created_by: Option<Uuid>,
) -> Result<Vec<GroupInclude>, Error> {
    conn.transaction(|conn| {
        // Parent must not be a managed group
        let parent = groups::table.find(parent_id).first::<Group>(conn)?;
        if parent.external_source.is_some() {
            return Err(Error::DatabaseError(
                diesel::result::DatabaseErrorKind::CheckViolation,
                Box::new("Externally managed groups cannot be composite parents".to_string()),
            ));
        }

        // Filter out self-inclusion
        let child_ids: Vec<i32> = child_ids
            .into_iter()
            .filter(|&id| id != parent_id)
            .collect();

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
                    Box::new(
                        "Circular reference: one or more child groups already include the parent"
                            .to_string(),
                    ),
                ));
            }
        }

        // Delete existing includes
        diesel::delete(group_includes::table.filter(group_includes::parent_group_id.eq(parent_id)))
            .execute(conn)?;

        let inserted: Vec<GroupInclude> = if child_ids.is_empty() {
            Vec::new()
        } else {
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
                .get_results(conn)?
        };

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::GroupMembership,
                aggregate_id: parent_id.to_string(),
                op: SyncOp::Update,
                event_type: "group_membership.includes_set",
                data: json!({
                    "parent_group_id": parent_id,
                    "child_group_ids": child_ids,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(inserted)
    })
}

// ============================================================================
// Asset-Group Membership Operations
// ============================================================================

/// Get all devices in a group
pub fn get_devices_in_group(conn: &mut DbConnection, group_id: i32) -> QueryResult<Vec<Asset>> {
    asset_groups::table
        .filter(asset_groups::group_id.eq(group_id))
        .inner_join(assets::table.on(assets::id.eq(asset_groups::asset_id)))
        .select(assets::all_columns)
        .load(conn)
}

/// Get all groups for a device
pub fn get_groups_for_device(conn: &mut DbConnection, device_id: i32) -> QueryResult<Vec<Group>> {
    asset_groups::table
        .filter(asset_groups::asset_id.eq(device_id))
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
) -> QueryResult<AssetGroup> {
    conn.transaction(|conn| {
        // Check if already exists
        let existing = asset_groups::table
            .filter(asset_groups::asset_id.eq(device_id))
            .filter(asset_groups::group_id.eq(group_id))
            .first::<AssetGroup>(conn);

        if let Ok(membership) = existing {
            return Ok(membership);
        }

        let new_membership = NewAssetGroup {
            asset_id: device_id,
            group_id,
            created_by,
            external_source: external_source.map(String::from),
        };

        let membership: AssetGroup = diesel::insert_into(asset_groups::table)
            .values(&new_membership)
            .get_result(conn)?;

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::GroupMembership,
                aggregate_id: format!("{}:{}", group_id, device_id),
                op: SyncOp::Insert,
                event_type: "group_membership.device_added",
                data: json!({
                    "group_id": group_id,
                    "device_id": device_id,
                    "external_source": external_source,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(membership)
    })
}

/// Remove a device from a group
pub fn remove_device_from_group(
    conn: &mut DbConnection,
    device_id: i32,
    group_id: i32,
) -> QueryResult<usize> {
    conn.transaction(|conn| {
        let result = diesel::delete(
            asset_groups::table
                .filter(asset_groups::asset_id.eq(device_id))
                .filter(asset_groups::group_id.eq(group_id)),
        )
        .execute(conn)?;
        if result > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::GroupMembership,
                    aggregate_id: format!("{}:{}", group_id, device_id),
                    op: SyncOp::Delete,
                    event_type: "group_membership.device_removed",
                    data: json!({
                        "group_id": group_id,
                        "device_id": device_id,
                    }),
                    groups: sync_groups::workspace(),
                    causation_id: None,
                },
            )?;
        }
        Ok(result)
    })
}

/// Get device IDs for a group (simple list)
pub fn get_device_ids_for_group(conn: &mut DbConnection, group_id: i32) -> QueryResult<Vec<i32>> {
    asset_groups::table
        .filter(asset_groups::group_id.eq(group_id))
        .select(asset_groups::asset_id)
        .load(conn)
}

/// Get device IDs for a group that were synced from an external source
pub fn get_synced_device_ids_for_group(
    conn: &mut DbConnection,
    group_id: i32,
    external_source: &str,
) -> QueryResult<Vec<i32>> {
    asset_groups::table
        .filter(asset_groups::group_id.eq(group_id))
        .filter(asset_groups::external_source.eq(external_source))
        .select(asset_groups::asset_id)
        .load(conn)
}

/// Set all devices of a group (replaces existing non-synced devices)
/// Note: This only removes manually-added devices, not externally synced ones
pub fn set_group_devices(
    conn: &mut DbConnection,
    group_id: i32,
    device_ids: Vec<i32>,
    created_by: Option<Uuid>,
) -> QueryResult<Vec<AssetGroup>> {
    conn.transaction(|conn| {
        // Delete all existing devices that were NOT synced from an external source
        // This preserves Microsoft-synced device memberships
        diesel::delete(
            asset_groups::table
                .filter(asset_groups::group_id.eq(group_id))
                .filter(asset_groups::external_source.is_null()),
        )
        .execute(conn)?;

        // Add new devices (manually added, so no external_source)
        let new_memberships: Vec<NewAssetGroup> = device_ids
            .iter()
            .map(|device_id| NewAssetGroup {
                asset_id: *device_id,
                group_id,
                created_by,
                external_source: None,
            })
            .collect();

        if !new_memberships.is_empty() {
            // Use ON CONFLICT DO NOTHING to handle devices that are already in the group via sync
            diesel::insert_into(asset_groups::table)
                .values(&new_memberships)
                .on_conflict((asset_groups::asset_id, asset_groups::group_id))
                .do_nothing()
                .execute(conn)?;
        }

        // Return the current state of device memberships
        let current: Vec<AssetGroup> = asset_groups::table
            .filter(asset_groups::group_id.eq(group_id))
            .load(conn)?;

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::GroupMembership,
                aggregate_id: group_id.to_string(),
                op: SyncOp::Update,
                event_type: "group_membership.devices_set",
                data: json!({
                    "group_id": group_id,
                    "device_ids": device_ids,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(current)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

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
