use crate::models::{NewUserTicketView, RecentTicket, UpdateUserTicketView, UserTicketView};
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

    /// Get recent tickets for a user
    pub fn get_recent_tickets(
        &self,
        user_uuid_param: Uuid,
        limit: i64,
    ) -> Result<Vec<RecentTicket>, diesel::result::Error> {
        use crate::schema::tickets;
        use crate::schema::user_ticket_views;

        let mut conn = self.pool.get().expect("Failed to get DB connection");

        // Join user_ticket_views with tickets, ordered by last_viewed_at
        user_ticket_views::table
            .inner_join(tickets::table.on(user_ticket_views::ticket_id.eq(tickets::id)))
            .filter(user_ticket_views::user_uuid.eq(user_uuid_param))
            .order(user_ticket_views::last_viewed_at.desc())
            .limit(limit)
            .select((
                tickets::id,
                tickets::title,
                tickets::status,
                tickets::requester_uuid,
                tickets::assignee_uuid,
                tickets::created_at,
                tickets::updated_at,
                user_ticket_views::last_viewed_at,
                user_ticket_views::view_count,
            ))
            .load::<(
                i32,
                String,
                crate::models::TicketStatus,
                Option<Uuid>,
                Option<Uuid>,
                chrono::NaiveDateTime,
                chrono::NaiveDateTime,
                chrono::NaiveDateTime,
                i32,
            )>(&mut conn)
            .map(|results| {
                results
                    .into_iter()
                    .map(
                        |(
                            tid,
                            ttitle,
                            tstatus,
                            req,
                            ass,
                            created,
                            updated,
                            last_viewed,
                            views,
                        )| RecentTicket {
                            id: tid,
                            title: ttitle,
                            status: tstatus,
                            requester: req,
                            assignee: ass,
                            created_at: created,
                            updated_at: updated,
                            last_viewed_at: last_viewed,
                            view_count: views,
                        },
                    )
                    .collect()
            })
    }

    /// Get all views for a specific user
    pub fn get_user_views(
        &self,
        user_uuid_param: Uuid,
    ) -> Result<Vec<UserTicketView>, diesel::result::Error> {
        use crate::schema::user_ticket_views::dsl::*;
        let mut conn = self.pool.get().expect("Failed to get DB connection");

        user_ticket_views
            .filter(user_uuid.eq(user_uuid_param))
            .order(last_viewed_at.desc())
            .load::<UserTicketView>(&mut conn)
    }

    /// Delete a view record (for cleanup)
    pub fn delete_view(
        &self,
        view_id: i32,
    ) -> Result<usize, diesel::result::Error> {
        use crate::schema::user_ticket_views::dsl::*;
        let mut conn = self.pool.get().expect("Failed to get DB connection");

        diesel::delete(user_ticket_views.find(view_id)).execute(&mut conn)
    }
}
