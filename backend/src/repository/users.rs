use diesel::prelude::*;
use diesel::result::Error;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

// User repository functions
pub fn get_users(conn: &mut DbConnection) -> Result<Vec<User>, Error> {
    users::table
        .order_by(users::name.asc())
        .load::<User>(conn)
}

// Get paginated users with filtering and sorting
pub fn get_paginated_users(
    conn: &mut DbConnection,
    page: i64,
    page_size: i64,
    sort_field: Option<String>,
    sort_direction: Option<String>,
    search: Option<String>,
    role: Option<String>,
) -> Result<(Vec<User>, i64), Error> {
    use crate::schema::user_emails;

    // Email search requires join with user_emails
    let has_search = search.as_ref().map(|s| !s.is_empty()).unwrap_or(false);

    if has_search {
        // Join with user_emails to search by email
        let search_term = search.as_ref().unwrap();
        let search_pattern = format!("%{}%", search_term.to_lowercase());

        // Get distinct user UUIDs that match the search (by name OR by any email)
        // Note: Explicit join condition needed because user_emails has two FKs to users
        let matching_uuids: Vec<Uuid> = users::table
            .left_join(user_emails::table.on(user_emails::user_uuid.eq(users::uuid)))
            .select(users::uuid)
            .filter(
                users::name.ilike(search_pattern.clone())
                    .or(user_emails::email.ilike(search_pattern.clone()))
            )
            .distinct()
            .load::<Uuid>(conn)?;

        // Now build the main query filtering by those UUIDs
        let mut query = users::table.into_boxed();
        query = query.filter(users::uuid.eq_any(&matching_uuids));

        // Handle role filter
        if let Some(role_filter) = role.clone() {
            if role_filter != "all" {
                let user_role = match role_filter.as_str() {
                    "admin" => UserRole::Admin,
                    "technician" => UserRole::Technician,
                    "user" => UserRole::User,
                    _ => UserRole::User,
                };
                query = query.filter(users::role.eq(user_role));
            }
        }

        // Count total matching records
        let mut count_query = users::table.into_boxed();
        count_query = count_query.filter(users::uuid.eq_any(&matching_uuids));
        if let Some(role_filter) = role {
            if role_filter != "all" {
                let user_role = match role_filter.as_str() {
                    "admin" => UserRole::Admin,
                    "technician" => UserRole::Technician,
                    "user" => UserRole::User,
                    _ => UserRole::User,
                };
                count_query = count_query.filter(users::role.eq(user_role));
            }
        }
        let total: i64 = count_query.count().get_result(conn)?;

        // Apply sorting
        match (sort_field.as_deref(), sort_direction.as_deref()) {
            (Some("name"), Some("asc")) => query = query.order(users::name.asc()),
            (Some("name"), _) => query = query.order(users::name.desc()),
            (Some("role"), Some("asc")) => query = query.order(users::role.asc()),
            (Some("role"), _) => query = query.order(users::role.desc()),
            _ => query = query.order(users::name.asc()),
        }

        // Apply pagination
        let offset = (page - 1) * page_size;
        query = query.offset(offset).limit(page_size);

        let results = query.load::<User>(conn)?;
        Ok((results, total))
    } else {
        // No search - simple query without join
        let mut query = users::table.into_boxed();

        // Handle role filter
        if let Some(role_filter) = role.clone() {
            if role_filter != "all" {
                let user_role = match role_filter.as_str() {
                    "admin" => UserRole::Admin,
                    "technician" => UserRole::Technician,
                    "user" => UserRole::User,
                    _ => UserRole::User,
                };
                query = query.filter(users::role.eq(user_role));
            }
        }

        // Build count query
        let mut count_query = users::table.into_boxed();
        if let Some(role_filter) = role {
            if role_filter != "all" {
                let user_role = match role_filter.as_str() {
                    "admin" => UserRole::Admin,
                    "technician" => UserRole::Technician,
                    "user" => UserRole::User,
                    _ => UserRole::User,
                };
                count_query = count_query.filter(users::role.eq(user_role));
            }
        }
        let total: i64 = count_query.count().get_result(conn)?;

        // Apply sorting
        match (sort_field.as_deref(), sort_direction.as_deref()) {
            (Some("name"), Some("asc")) => query = query.order(users::name.asc()),
            (Some("name"), _) => query = query.order(users::name.desc()),
            (Some("role"), Some("asc")) => query = query.order(users::role.asc()),
            (Some("role"), _) => query = query.order(users::role.desc()),
            _ => query = query.order(users::name.asc()),
        }

        // Apply pagination
        let offset = (page - 1) * page_size;
        query = query.offset(offset).limit(page_size);

        let results = query.load::<User>(conn)?;
        Ok((results, total))
    }
}

// Note: get_user_by_id removed - users table now uses UUID as primary key
// Use get_user_by_uuid instead
pub fn get_user_by_uuid(uuid: &Uuid, conn: &mut DbConnection) -> Result<User, Error> {
    users::table
        .find(uuid)
        .first::<User>(conn)
}

/// Get user by Microsoft UUID
pub fn get_user_by_microsoft_uuid(conn: &mut DbConnection, microsoft_uuid: &Uuid) -> QueryResult<User> {
    users::table.filter(users::microsoft_uuid.eq(microsoft_uuid)).first(conn)
}

// This function now delegates to user_helpers module since email is in user_emails table
pub fn get_user_by_email(email: &str, conn: &mut DbConnection) -> Result<User, Error> {
    crate::repository::user_helpers::get_user_by_email(email, conn)
}

pub fn get_user_by_name(name: &str, conn: &mut DbConnection) -> Result<User, Error> {
    users::table
        .filter(users::name.eq(name))
        .first::<User>(conn)
}

pub fn create_user(
    user: NewUser,
    conn: &mut DbConnection,
) -> Result<User, Error> {
    diesel::insert_into(users::table)
        .values(user)
        .get_result(conn)
}

pub fn update_user(
    user_uuid: &Uuid,
    user: UserUpdate,
    conn: &mut DbConnection,
) -> Result<User, Error> {
    diesel::update(users::table.find(user_uuid))
        .set(user)
        .get_result(conn)
}

pub fn delete_user(user_uuid: &Uuid, conn: &mut DbConnection) -> Result<usize, Error> {
    use crate::schema::{
        comments, devices, tickets, attachments, linked_tickets, project_tickets,
        ticket_devices, article_contents, sync_history, user_auth_identities, user_emails,
        documentation_pages, documentation_revisions, projects,
    };

    // Start a transaction to ensure all-or-nothing deletion
    conn.transaction::<_, Error, _>(|conn| {
        // === Phase 1: Handle RESTRICT constraints ===
        // Tables with ON DELETE RESTRICT require explicit deletion/reassignment first

        // 1a. Delete all comments by this user
        diesel::delete(comments::table.filter(comments::user_uuid.eq(user_uuid)))
            .execute(conn)?;

        // 1b. Update tickets where user is requester (RESTRICT) - set to NULL
        diesel::update(tickets::table.filter(tickets::requester_uuid.eq(user_uuid)))
            .set(tickets::requester_uuid.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 1c. Update documentation_pages created_by/last_edited_by (RESTRICT)
        // Reassign to first available admin
        let first_admin: Option<User> = users::table
            .into_boxed()
            .filter(users::role.eq(crate::models::UserRole::Admin))
            .filter(users::uuid.ne(user_uuid))
            .first(conn)
            .optional()?;

        if let Some(admin) = first_admin {
            // Reassign documentation to another admin
            diesel::update(documentation_pages::table.filter(documentation_pages::created_by.eq(user_uuid)))
                .set(documentation_pages::created_by.eq(admin.uuid))
                .execute(conn)?;

            diesel::update(documentation_pages::table.filter(documentation_pages::last_edited_by.eq(user_uuid)))
                .set(documentation_pages::last_edited_by.eq(admin.uuid))
                .execute(conn)?;

            diesel::update(documentation_revisions::table.filter(documentation_revisions::created_by.eq(user_uuid)))
                .set(documentation_revisions::created_by.eq(admin.uuid))
                .execute(conn)?;
        }
        // Note: If no other admin exists, delete fails due to FK constraint
        // Intentional - at least one admin must own documentation

        // === Phase 2: Handle SET NULL constraints ===
        // These tables have ON DELETE SET NULL but handled explicitly for clarity

        // 2a. Devices
        diesel::update(devices::table.filter(devices::primary_user_uuid.eq(user_uuid)))
            .set(devices::primary_user_uuid.eq::<Option<Uuid>>(None))
            .execute(conn)?;
        diesel::update(devices::table.filter(devices::created_by.eq(user_uuid)))
            .set(devices::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2b. Tickets (assignee, created_by, closed_by)
        diesel::update(tickets::table.filter(tickets::assignee_uuid.eq(user_uuid)))
            .set(tickets::assignee_uuid.eq::<Option<Uuid>>(None))
            .execute(conn)?;
        diesel::update(tickets::table.filter(tickets::created_by.eq(user_uuid)))
            .set(tickets::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;
        diesel::update(tickets::table.filter(tickets::closed_by.eq(user_uuid)))
            .set(tickets::closed_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2c. Projects
        diesel::update(projects::table.filter(projects::created_by.eq(user_uuid)))
            .set(projects::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;
        diesel::update(projects::table.filter(projects::owner_uuid.eq(user_uuid)))
            .set(projects::owner_uuid.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2d. Attachments
        diesel::update(attachments::table.filter(attachments::uploaded_by.eq(user_uuid)))
            .set(attachments::uploaded_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2e. Linked tickets
        diesel::update(linked_tickets::table.filter(linked_tickets::created_by.eq(user_uuid)))
            .set(linked_tickets::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2f. Project tickets
        diesel::update(project_tickets::table.filter(project_tickets::created_by.eq(user_uuid)))
            .set(project_tickets::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2g. Ticket devices
        diesel::update(ticket_devices::table.filter(ticket_devices::created_by.eq(user_uuid)))
            .set(ticket_devices::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2h. Article contents
        diesel::update(article_contents::table.filter(article_contents::created_by.eq(user_uuid)))
            .set(article_contents::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;
        diesel::update(article_contents::table.filter(article_contents::updated_by.eq(user_uuid)))
            .set(article_contents::updated_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2i. Sync history
        diesel::update(sync_history::table.filter(sync_history::initiated_by.eq(user_uuid)))
            .set(sync_history::initiated_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2j. User auth identities created_by
        diesel::update(user_auth_identities::table.filter(user_auth_identities::created_by.eq(user_uuid)))
            .set(user_auth_identities::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2k. User emails created_by
        diesel::update(user_emails::table.filter(user_emails::created_by.eq(user_uuid)))
            .set(user_emails::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // === Phase 3: Delete CASCADE tables explicitly (for clarity) ===
        // These would be deleted automatically by CASCADE, but explicit is clearer

        // 3a. Delete user auth identities
        diesel::delete(user_auth_identities::table.filter(user_auth_identities::user_uuid.eq(user_uuid)))
            .execute(conn)?;

        // 3b. Delete user emails
        diesel::delete(user_emails::table.filter(user_emails::user_uuid.eq(user_uuid)))
            .execute(conn)?;

        // === Phase 4: Delete the user ===
        let deleted_count = diesel::delete(users::table.find(user_uuid)).execute(conn)?;

        Ok(deleted_count)
    })
}

// Batch get users by UUIDs
pub fn get_users_by_uuids(uuids: &[Uuid], conn: &mut DbConnection) -> Result<Vec<User>, Error> {
    users::table
        .filter(users::uuid.eq_any(uuids))
        .order_by(users::name.asc())
        .load::<User>(conn)
}

// Count total users in the database (for onboarding check)
pub fn count_users(conn: &mut DbConnection) -> Result<i64, Error> {
    users::table.count().get_result(conn)
}

/// Update user MFA fields by UUID
pub fn update_user_mfa(
    uuid: &Uuid,
    mfa_update: UserMfaUpdate,
    conn: &mut DbConnection,
) -> Result<User, Error> {
    diesel::update(users::table.filter(users::uuid.eq(uuid)))
        .set(mfa_update)
        .get_result(conn)
}

/// Update user passkey credentials by UUID
pub fn update_user_passkey_credentials(
    conn: &mut DbConnection,
    uuid: &Uuid,
    passkey_credentials: Option<serde_json::Value>,
) -> Result<User, Error> {
    use crate::models::UserPasskeyUpdate;

    let update = UserPasskeyUpdate {
        passkey_credentials,
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    diesel::update(users::table.filter(users::uuid.eq(uuid)))
        .set(update)
        .get_result(conn)
} 