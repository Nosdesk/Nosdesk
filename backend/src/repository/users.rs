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
    // Build the main query
    let mut query = users::table.into_boxed();
    
    // Apply filters if provided
    // Note: Email search removed - email is now in user_emails table
    if let Some(search_term) = search.clone() {
        if !search_term.is_empty() {
            let search_pattern = format!("%{}%", search_term.to_lowercase());
            // Note: ID-based search removed since users table now uses UUID primary key
            query = query.filter(users::name.ilike(search_pattern.clone()));
        }
    }
    
    // Handle role filter - convert string to UserRole enum
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
    
    // Build a separate count query with the same filters
    let mut count_query = users::table.into_boxed();
    
    // Apply the same filters to the count query
    if let Some(search_term) = search {
        if !search_term.is_empty() {
            let search_pattern = format!("%{}%", search_term.to_lowercase());
            // Note: ID-based search removed since users table now uses UUID primary key
            count_query = count_query.filter(users::name.ilike(search_pattern.clone()));
        }
    }
    
    // Handle role filter for count query
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
    
    // Count total matching records (before pagination)
    let total: i64 = count_query.count().get_result(conn)?;
    
    // Apply sorting to the main query
    // Note: Email sorting removed - would require join with user_emails table
    // Note: ID sorting removed - users table now uses UUID primary key (not sortable by id)
    match (sort_field.as_deref(), sort_direction.as_deref()) {
        (Some("name"), Some("asc")) => query = query.order(users::name.asc()),
        (Some("name"), _) => query = query.order(users::name.desc()),
        (Some("email"), Some("asc")) => query = query.order(users::name.asc()), // Fallback to name
        (Some("email"), _) => query = query.order(users::name.desc()), // Fallback to name
        (Some("role"), Some("asc")) => query = query.order(users::role.asc()),
        (Some("role"), _) => query = query.order(users::role.desc()),
        _ => query = query.order(users::name.asc()), // Default sort by name
    }
    
    // Apply pagination
    let offset = (page - 1) * page_size;
    query = query.offset(offset).limit(page_size);
    
    // Execute the query
    let results = query.load::<User>(conn)?;
    
    Ok((results, total))
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
    use crate::schema::{comments, devices, user_auth_identities, user_emails};

    // Start a transaction to ensure all-or-nothing deletion
    conn.transaction::<_, Error, _>(|conn| {
        // 1. Delete all comments by this user
        // Note: comments.user_uuid references users.uuid
        diesel::delete(comments::table.filter(comments::user_uuid.eq(user_uuid)))
            .execute(conn)?;

        // 2. Update devices to remove user associations (set primary_user_uuid to NULL)
        // Note: devices.primary_user_uuid references users.uuid
        diesel::update(devices::table.filter(devices::primary_user_uuid.eq(user_uuid)))
            .set(devices::primary_user_uuid.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 3. Delete user auth identities
        // Note: user_auth_identities.user_uuid references users.uuid with ON DELETE CASCADE
        diesel::delete(user_auth_identities::table.filter(user_auth_identities::user_uuid.eq(user_uuid)))
            .execute(conn)?;

        // 4. Delete user emails
        // Note: user_emails.user_uuid references users.uuid with ON DELETE CASCADE
        diesel::delete(user_emails::table.filter(user_emails::user_uuid.eq(user_uuid)))
            .execute(conn)?;

        // 5. Finally delete the user
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