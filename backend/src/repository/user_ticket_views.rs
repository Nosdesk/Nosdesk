use crate::models::{NewUserTicketView, RecentTicket, UpdateUserTicketView, UserTicketView};

/// How many recent-tickets rows the sidebar / dashboard widget
/// fetch in one go. Surfaced as a constant so the handler and
/// any future consumer agree without a magic number drifting
/// between them.
pub const RECENT_TICKETS_LIMIT: i64 = 15;
use chrono::Utc;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use uuid::Uuid;

pub struct UserTicketViewsRepository {
    pool: Pool<ConnectionManager<PgConnection>>,
}

impl UserTicketViewsRepository {
    pub fn new(pool: Pool<ConnectionManager<PgConnection>>) -> Self {
        UserTicketViewsRepository { pool }
    }

    // sync-audit-only: Operational / bespoke tables
    /// Record a ticket view - either insert new or update existing
    pub fn record_view(
        &self,
        user_uuid_param: Uuid,
        ticket_id_param: i32,
    ) -> Result<UserTicketView, diesel::result::Error> {
        use crate::schema::user_ticket_views::dsl::*;
        let mut conn = self.pool.get().expect("Failed to get DB connection");

        // Try to find existing view record
        let existing = user_ticket_views
            .filter(user_uuid.eq(user_uuid_param))
            .filter(ticket_id.eq(ticket_id_param))
            .first::<UserTicketView>(&mut conn)
            .optional()?;

        if let Some(view) = existing {
            // Update existing record
            let update = UpdateUserTicketView {
                last_viewed_at: Utc::now().naive_utc(),
                view_count: view.view_count + 1,
            };

            diesel::update(user_ticket_views.find(view.id))
                .set(&update)
                .get_result(&mut conn)
        } else {
            // Insert new record
            let new_view = NewUserTicketView {
                user_uuid: user_uuid_param,
                ticket_id: ticket_id_param,
            };

            diesel::insert_into(user_ticket_views)
                .values(&new_view)
                .get_result(&mut conn)
        }
    }

    // sync-audit-only: Operational / bespoke tables
    /// Delete a ticket view record for a user
    pub fn delete_view(
        &self,
        user_uuid_param: Uuid,
        ticket_id_param: i32,
    ) -> Result<usize, diesel::result::Error> {
        use crate::schema::user_ticket_views::dsl::*;
        let mut conn = self.pool.get().expect("Failed to get DB connection");

        diesel::delete(
            user_ticket_views
                .filter(user_uuid.eq(user_uuid_param))
                .filter(ticket_id.eq(ticket_id_param)),
        )
        .execute(&mut conn)
    }

    /// Get recent tickets for a user.
    ///
    /// One JOIN to `workflow_states` so the category enum lands
    /// alongside the row in a single query. The previous loop
    /// asked `category_of` per row; even though that helper is
    /// cached, the lookup-and-RwLock-grab on each iteration was
    /// avoidable noise. With the category pulled directly,
    /// nothing in the result-mapping loop touches the DB or the
    /// cache.
    pub fn get_recent_tickets(
        &self,
        user_uuid_param: Uuid,
        limit: i64,
    ) -> Result<Vec<RecentTicket>, diesel::result::Error> {
        use crate::models::WorkflowStateCategory;
        use crate::schema::{tickets, user_ticket_views, workflow_states};

        let mut conn = self.pool.get().expect("Failed to get DB connection");

        let rows: Vec<(
            i32,
            String,
            i32,
            Option<WorkflowStateCategory>,
            Option<Uuid>,
            Option<Uuid>,
            chrono::NaiveDateTime,
            chrono::NaiveDateTime,
            chrono::NaiveDateTime,
            i32,
        )> = user_ticket_views::table
            .inner_join(tickets::table.on(user_ticket_views::ticket_id.eq(tickets::id)))
            .left_join(
                workflow_states::table.on(tickets::workflow_state_id.eq(workflow_states::id)),
            )
            .filter(user_ticket_views::user_uuid.eq(user_uuid_param))
            .order(user_ticket_views::last_viewed_at.desc())
            .limit(limit)
            .select((
                tickets::id,
                tickets::title,
                tickets::workflow_state_id,
                workflow_states::category.nullable(),
                tickets::requester_uuid,
                tickets::assignee_uuid,
                tickets::created_at,
                tickets::updated_at,
                user_ticket_views::last_viewed_at,
                user_ticket_views::view_count,
            ))
            .load(&mut conn)?;

        Ok(rows
            .into_iter()
            .map(
                |(tid, ttitle, ws_id, cat, req, ass, created, updated, last_viewed, views)| {
                    let cat = cat.unwrap_or(WorkflowStateCategory::Backlog);
                    RecentTicket {
                        id: tid,
                        title: ttitle,
                        status: cat.legacy_status().to_string(),
                        workflow_state_id: ws_id,
                        requester: req,
                        assignee: ass,
                        created_at: created,
                        updated_at: updated,
                        last_viewed_at: last_viewed,
                        view_count: views,
                    }
                },
            )
            .collect())
    }
}
