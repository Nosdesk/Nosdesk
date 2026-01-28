//! Ticket Query Builder
//!
//! Provides a fluent, type-safe API for building ticket queries with
//! automatic permission filtering.

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::extractors::AuthContext;
use crate::models::{Ticket, TicketListItem, TicketPriority, TicketStatus, UserInfoWithAvatar};
use crate::schema::tickets;

/// Parse comma-separated status filter into enums
fn parse_status_filter(status_str: &str) -> Vec<TicketStatus> {
    status_str
        .split(',')
        .filter_map(|s| match s.trim().to_lowercase().as_str() {
            "open" => Some(TicketStatus::Open),
            "in-progress" => Some(TicketStatus::InProgress),
            "closed" => Some(TicketStatus::Closed),
            _ => None,
        })
        .collect()
}

/// Parse priority string to enum
fn parse_priority(priority_str: &str) -> TicketPriority {
    match priority_str.to_lowercase().as_str() {
        "low" => TicketPriority::Low,
        "high" => TicketPriority::High,
        _ => TicketPriority::Medium,
    }
}

/// Builder for constructing ticket queries with fluent API
#[derive(Default)]
pub struct TicketQuery {
    // Visibility filters
    visible_to_user: Option<Uuid>,
    visible_to_groups: Vec<i32>,
    visible_category_ids: Option<Vec<i32>>,

    // Content filters
    search: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    category_id: Option<i32>,
    assignee_uuid: Option<Uuid>,
    assignee_unassigned: bool,
    requester_uuid: Option<Uuid>,

    // Date filters
    created_after: Option<chrono::NaiveDateTime>,
    created_before: Option<chrono::NaiveDateTime>,
    modified_after: Option<chrono::NaiveDateTime>,
    modified_before: Option<chrono::NaiveDateTime>,
    closed_after: Option<chrono::NaiveDateTime>,
    closed_before: Option<chrono::NaiveDateTime>,

    // Pagination & sorting
    page: i64,
    page_size: i64,
    sort_field: Option<String>,
    sort_direction: Option<String>,
}

impl TicketQuery {
    /// Create a new ticket query builder
    pub fn new() -> Self {
        Self {
            page: 1,
            page_size: 10,
            ..Default::default()
        }
    }

    /// Apply visibility rules based on auth context
    ///
    /// For regular users: only shows tickets they requested or are assigned to
    /// For technicians/admins: shows all tickets (no filter applied)
    /// For group-based access: includes tickets visible to user's groups (future)
    pub fn visible_to(mut self, auth: &AuthContext) -> Self {
        if !auth.is_technician_or_admin() {
            self.visible_to_user = Some(auth.user_uuid);
            self.visible_to_groups = auth.group_ids.clone();
        }
        self
    }

    /// Search in title and ticket ID
    pub fn search(mut self, term: Option<String>) -> Self {
        self.search = term.filter(|s| !s.is_empty());
        self
    }

    /// Filter by status (comma-separated for multiple)
    pub fn status(mut self, status: Option<String>) -> Self {
        self.status = status.filter(|s| s != "all");
        self
    }

    /// Filter by priority
    pub fn priority(mut self, priority: Option<String>) -> Self {
        self.priority = priority.filter(|p| p != "all");
        self
    }

    /// Filter by category
    pub fn category(mut self, category: Option<String>) -> Self {
        if let Some(cat) = category {
            if cat != "all" {
                self.category_id = cat.parse().ok();
            }
        }
        self
    }

    /// Filter by assignee UUID
    pub fn assignee(mut self, assignee: Option<String>) -> Self {
        if let Some(a) = assignee {
            if a == "unassigned" {
                self.assignee_unassigned = true;
            } else if a != "all" {
                self.assignee_uuid = Uuid::parse_str(&a).ok();
            }
        }
        self
    }

    /// Filter by requester UUID
    pub fn requester(mut self, requester: Option<String>) -> Self {
        if let Some(r) = requester {
            if r != "all" {
                self.requester_uuid = Uuid::parse_str(&r).ok();
            }
        }
        self
    }

    /// Filter by creation date range
    pub fn created_between(mut self, after: Option<String>, before: Option<String>) -> Self {
        if let Some(date_str) = after {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                self.created_after = date.and_hms_opt(0, 0, 0);
            }
        }
        if let Some(date_str) = before {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                self.created_before = date.and_hms_opt(23, 59, 59);
            }
        }
        self
    }

    /// Filter by creation date (exact day)
    pub fn created_on(mut self, date: Option<String>) -> Self {
        if let Some(date_str) = date {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                self.created_after = date.and_hms_opt(0, 0, 0);
                self.created_before = date.and_hms_opt(23, 59, 59);
            }
        }
        self
    }

    /// Filter by modification date range
    pub fn modified_between(mut self, after: Option<String>, before: Option<String>) -> Self {
        if let Some(date_str) = after {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                self.modified_after = date.and_hms_opt(0, 0, 0);
            }
        }
        if let Some(date_str) = before {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                self.modified_before = date.and_hms_opt(23, 59, 59);
            }
        }
        self
    }

    /// Filter by modification date (exact day)
    pub fn modified_on(mut self, date: Option<String>) -> Self {
        if let Some(date_str) = date {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                self.modified_after = date.and_hms_opt(0, 0, 0);
                self.modified_before = date.and_hms_opt(23, 59, 59);
            }
        }
        self
    }

    /// Filter by closed date range
    pub fn closed_between(mut self, after: Option<String>, before: Option<String>) -> Self {
        if let Some(date_str) = after {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                self.closed_after = date.and_hms_opt(0, 0, 0);
            }
        }
        if let Some(date_str) = before {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                self.closed_before = date.and_hms_opt(23, 59, 59);
            }
        }
        self
    }

    /// Filter by closed date (exact day)
    pub fn closed_on(mut self, date: Option<String>) -> Self {
        if let Some(date_str) = date {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                self.closed_after = date.and_hms_opt(0, 0, 0);
                self.closed_before = date.and_hms_opt(23, 59, 59);
            }
        }
        self
    }

    /// Set pagination
    pub fn paginate(mut self, page: i64, page_size: i64) -> Self {
        self.page = page.max(1);
        self.page_size = page_size.clamp(1, 100);
        self
    }

    /// Set sorting
    pub fn sort(mut self, field: Option<String>, direction: Option<String>) -> Self {
        self.sort_field = field;
        self.sort_direction = direction;
        self
    }

    /// Resolve visible category IDs for a non-admin user based on group memberships.
    /// Queries category_group_visibility to find which categories the user's groups can access,
    /// plus all public categories (those with no group restrictions).
    fn resolve_visibility(&mut self, conn: &mut DbConnection) {
        use crate::schema::category_group_visibility;
        use crate::schema::ticket_categories;

        if self.visible_to_user.is_none() {
            return; // Admin/tech — no filtering needed
        }

        // Get category IDs accessible via user's groups
        let group_category_ids: Vec<i32> = if !self.visible_to_groups.is_empty() {
            category_group_visibility::table
                .filter(category_group_visibility::group_id.eq_any(&self.visible_to_groups))
                .select(category_group_visibility::category_id)
                .distinct()
                .load(conn)
                .unwrap_or_default()
        } else {
            vec![]
        };

        // Get public category IDs (no entries in category_group_visibility)
        // A category is public if it has zero rows in category_group_visibility
        let restricted_category_ids: Vec<i32> = category_group_visibility::table
            .select(category_group_visibility::category_id)
            .distinct()
            .load(conn)
            .unwrap_or_default();

        let public_category_ids: Vec<i32> = ticket_categories::table
            .filter(ticket_categories::is_active.eq(true))
            .filter(diesel::dsl::not(ticket_categories::id.eq_any(&restricted_category_ids)))
            .select(ticket_categories::id)
            .load(conn)
            .unwrap_or_default();

        // Combine: public + group-accessible
        let mut visible: Vec<i32> = public_category_ids;
        for id in group_category_ids {
            if !visible.contains(&id) {
                visible.push(id);
            }
        }

        self.visible_category_ids = Some(visible);
    }

    /// Build and apply all filters to a boxed query
    fn apply_filters<'a>(
        &self,
        mut query: tickets::BoxedQuery<'a, diesel::pg::Pg>,
    ) -> tickets::BoxedQuery<'a, diesel::pg::Pg> {
        // Visibility filter - user sees own tickets + tickets in visible categories
        if let Some(user_uuid) = self.visible_to_user {
            if let Some(ref visible_cats) = self.visible_category_ids {
                // User can see: their own tickets OR tickets in visible categories OR uncategorized tickets
                query = query.filter(
                    tickets::requester_uuid.eq(Some(user_uuid))
                        .or(tickets::assignee_uuid.eq(Some(user_uuid)))
                        .or(tickets::category_id.is_null())
                        .or(tickets::category_id.eq_any(
                            visible_cats.iter().map(|&id| Some(id)).collect::<Vec<Option<i32>>>()
                        ))
                );
            } else {
                // No visibility resolved (shouldn't happen), fall back to own tickets only
                query = query.filter(
                    tickets::requester_uuid.eq(Some(user_uuid))
                        .or(tickets::assignee_uuid.eq(Some(user_uuid)))
                );
            }
        }

        // Search filter
        if let Some(ref search_term) = self.search {
            let pattern = format!("%{}%", search_term.to_lowercase());
            query = query.filter(
                tickets::title.ilike(pattern.clone())
                    .or(tickets::id.eq_any(
                        search_term.parse::<i32>().ok().map(|id| vec![id]).unwrap_or_default()
                    ))
            );
        }

        // Status filter
        if let Some(ref status_str) = self.status {
            let statuses = parse_status_filter(status_str);
            if !statuses.is_empty() {
                query = query.filter(tickets::status.eq_any(statuses));
            }
        }

        // Priority filter
        if let Some(ref priority_str) = self.priority {
            let priority = parse_priority(priority_str);
            query = query.filter(tickets::priority.eq(priority));
        }

        // Category filter
        if let Some(category_id) = self.category_id {
            query = query.filter(tickets::category_id.eq(Some(category_id)));
        }

        // Assignee filter
        if self.assignee_unassigned {
            query = query.filter(tickets::assignee_uuid.is_null());
        } else if let Some(assignee) = self.assignee_uuid {
            query = query.filter(tickets::assignee_uuid.eq(Some(assignee)));
        }

        // Requester filter
        if let Some(requester) = self.requester_uuid {
            query = query.filter(tickets::requester_uuid.eq(Some(requester)));
        }

        // Date filters
        if let Some(dt) = self.created_after {
            query = query.filter(tickets::created_at.ge(dt));
        }
        if let Some(dt) = self.created_before {
            query = query.filter(tickets::created_at.le(dt));
        }
        if let Some(dt) = self.modified_after {
            query = query.filter(tickets::updated_at.ge(dt));
        }
        if let Some(dt) = self.modified_before {
            query = query.filter(tickets::updated_at.le(dt));
        }
        if let Some(dt) = self.closed_after {
            query = query.filter(tickets::closed_at.gt(dt));
        }
        if let Some(dt) = self.closed_before {
            query = query.filter(tickets::closed_at.lt(dt));
        }

        query
    }

    /// Apply sorting to query
    fn apply_sorting<'a>(
        &self,
        mut query: tickets::BoxedQuery<'a, diesel::pg::Pg>,
    ) -> tickets::BoxedQuery<'a, diesel::pg::Pg> {
        match (self.sort_field.as_deref(), self.sort_direction.as_deref()) {
            (Some("id"), Some("asc")) => query = query.order(tickets::id.asc()),
            (Some("id"), _) => query = query.order(tickets::id.desc()),
            (Some("title"), Some("asc")) => query = query.order(tickets::title.asc()),
            (Some("title"), _) => query = query.order(tickets::title.desc()),
            (Some("status"), Some("asc")) => query = query.order(tickets::status.asc()),
            (Some("status"), _) => query = query.order(tickets::status.desc()),
            (Some("priority"), Some("asc")) => query = query.order(tickets::priority.asc()),
            (Some("priority"), _) => query = query.order(tickets::priority.desc()),
            (Some("created_at"), Some("asc")) => query = query.order(tickets::created_at.asc()),
            (Some("created_at"), _) => query = query.order(tickets::created_at.desc()),
            _ => query = query.order(tickets::id.desc()),
        }
        query
    }

    /// Execute the query and return paginated results with user info
    pub fn execute_with_users(
        mut self,
        conn: &mut DbConnection,
    ) -> Result<PaginatedResult<TicketListItem>, diesel::result::Error> {
        // Resolve category visibility for non-admin users
        self.resolve_visibility(conn);

        // Build count query
        let count_query = self.apply_filters(tickets::table.into_boxed());
        let total: i64 = count_query.count().get_result(conn)?;

        // Build main query with sorting and pagination
        let mut query = self.apply_filters(tickets::table.into_boxed());
        query = self.apply_sorting(query);

        let offset = (self.page - 1) * self.page_size;
        query = query.offset(offset).limit(self.page_size);

        let tickets: Vec<Ticket> = query.load(conn)?;

        // Enrich with user information
        let items = tickets
            .into_iter()
            .map(|ticket| {
                let requester_user = ticket.requester_uuid.as_ref()
                    .and_then(|uuid| crate::repository::get_user_by_uuid(uuid, conn).ok())
                    .map(UserInfoWithAvatar::from);

                let assignee_user = ticket.assignee_uuid
                    .and_then(|uuid| crate::repository::get_user_by_uuid(&uuid, conn).ok())
                    .map(UserInfoWithAvatar::from);

                TicketListItem {
                    ticket,
                    requester_user,
                    assignee_user,
                }
            })
            .collect();

        let total_pages = (total as f64 / self.page_size as f64).ceil() as i64;

        Ok(PaginatedResult {
            data: items,
            total,
            page: self.page,
            page_size: self.page_size,
            total_pages,
        })
    }

}

/// Paginated query result
#[derive(Debug, serde::Serialize)]
pub struct PaginatedResult<T> {
    pub data: Vec<T>,
    pub total: i64,
    pub page: i64,
    #[serde(rename = "pageSize")]
    pub page_size: i64,
    #[serde(rename = "totalPages")]
    pub total_pages: i64,
}
