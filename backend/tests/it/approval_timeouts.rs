//! The automatic approval sweep (`scheduled_jobs::approval_timeouts`): it
//! approves only a current round nobody answered in time, and leaves a request
//! that was closed or cancelled while it waited alone.

use diesel::prelude::*;
use uuid::Uuid;

use backend::models::{NewComment, NewTicket, Ticket, TicketUpdate, WorkflowStateCategory};
use backend::repository::{categories, comments, ticket_approvals, tickets as ticket_repo};
use backend::repository::{user_contact, workflow_states};
use backend::schema::{ticket_approvals as approvals, ticket_categories, tickets};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common::{self, TestPool, WorkspaceSeed};

struct Fixture {
    _db: common::TestDb,
    pool: TestPool,
    ws: WorkspaceSeed,
}

impl Fixture {
    /// A workspace that approves automatically after three days.
    fn new() -> Self {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
        let fx = Self { _db: db, pool, ws };
        fx.run(|c| {
            backend::repository::site_settings::get_site_settings(c)?;
            diesel::sql_query("UPDATE site_settings SET approval_auto_approve_days = 3").execute(c)
        });
        fx
    }

    /// Run `f` in the workspace as its admin.
    fn run<T>(
        &self,
        f: impl FnOnce(&mut backend::db::DbConnection) -> diesel::QueryResult<T>,
    ) -> T {
        let mut conn = self.pool.get().expect("conn");
        let admin =
            ActorContext::user(self.ws.admin_uuid, None).with_workspace(self.ws.workspace_id);
        with_actor_context(&mut conn, &admin, f).expect("run in workspace")
    }

    /// A request from the member in a new type `name` the admin approves,
    /// waiting for its first round. With `manager`, the member's manager
    /// approves too.
    fn waiting_request(&self, name: &str, manager: Option<Uuid>) -> Ticket {
        self.run(|c| {
            if let Some(manager) = manager {
                user_contact::set_manager(c, self.ws.member_uuid, Some(manager), None)?;
            }
            let category: i32 = diesel::insert_into(ticket_categories::table)
                .values((
                    ticket_categories::name.eq(name),
                    ticket_categories::approval_required.eq(true),
                    ticket_categories::approval_rule.eq("all"),
                    ticket_categories::approval_by_manager.eq(manager.is_some()),
                ))
                .returning(ticket_categories::id)
                .get_result(c)?;
            categories::set_category_approvers(c, category, &[self.ws.admin_uuid])?;
            let open = workflow_states::default_state(c)?.id;
            let ticket: Ticket = diesel::insert_into(tickets::table)
                .values(&NewTicket {
                    title: name.to_string(),
                    workflow_state_id: open,
                    requester_uuid: Some(self.ws.member_uuid),
                    category_id: Some(category),
                    ..Default::default()
                })
                .get_result(c)?;
            ticket_approvals::start_if_required(c, &ticket)?;
            tickets::table.find(ticket.id).first(c)
        })
    }

    /// Make every row of `round` look `days` old.
    fn age_round(&self, ticket_id: i32, round: i32, days: i64) {
        self.run(|c| {
            diesel::update(
                approvals::table
                    .filter(approvals::ticket_id.eq(ticket_id))
                    .filter(approvals::round.eq(round)),
            )
            .set(approvals::created_at.eq(chrono::Utc::now() - chrono::Duration::days(days)))
            .execute(c)
        });
    }

    fn approval_state(&self, ticket_id: i32) -> Option<String> {
        self.run(|c| {
            tickets::table
                .find(ticket_id)
                .select(tickets::approval_state)
                .first(c)
        })
    }

    /// `ticket.approval_decided` events for the ticket: what the requester's
    /// "approved" or "declined" notice is derived from.
    fn decided_events(&self, ticket_id: i32) -> i64 {
        #[derive(QueryableByName)]
        struct Count {
            #[diesel(sql_type = diesel::sql_types::BigInt)]
            n: i64,
        }
        self.run(|c| {
            diesel::sql_query(
                "SELECT count(*) AS n FROM sync_actions \
                 WHERE event_type = 'ticket.approval_decided' AND aggregate_id = $1",
            )
            .bind::<diesel::sql_types::Text, _>(ticket_id.to_string())
            .get_result::<Count>(c)
        })
        .n
    }

    async fn sweep(&self) {
        backend::services::scheduled_jobs::approval_timeouts(self.pool.clone())
            .await
            .expect("run the job");
    }
}

/// A declined request the requester replies to asks again, and the new round
/// waits its own period: the old round's unanswered row doesn't time it out.
#[actix_web::test]
async fn a_restarted_round_waits_its_own_period() {
    let fx = Fixture::new();
    let manager = {
        let mut conn = fx.pool.get().expect("conn");
        common::insert_plain_user(&mut conn, "The manager")
    };
    let ticket = fx.waiting_request("New laptop", Some(manager));

    // The admin declines on day one; the manager never answers.
    fx.run(|c| {
        ticket_approvals::decide(
            c,
            ticket.id,
            fx.ws.admin_uuid,
            false,
            Some("Not this quarter"),
            "portal",
            None,
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    });
    assert_eq!(fx.approval_state(ticket.id).as_deref(), Some("declined"));
    fx.age_round(ticket.id, 1, 10);

    // Ten days on, the requester replies, which reopens it and asks again.
    fx.run(|c| {
        comments::create_comment(
            c,
            NewComment {
                content: "Can we try again?".to_string(),
                ticket_id: ticket.id,
                user_uuid: fx.ws.member_uuid,
                ..Default::default()
            },
            None,
        )
    });
    assert_eq!(fx.approval_state(ticket.id).as_deref(), Some("pending"));
    let rounds: Vec<i32> = fx.run(|c| {
        approvals::table
            .filter(approvals::ticket_id.eq(ticket.id))
            .filter(approvals::decision.is_null())
            .select(approvals::round)
            .order(approvals::round.asc())
            .load(c)
    });
    assert_eq!(rounds, [1, 2, 2], "round 1's unanswered row is still there");

    fx.sweep().await;
    assert_eq!(
        fx.approval_state(ticket.id).as_deref(),
        Some("pending"),
        "the new round hasn't waited three days"
    );

    // Once the new round itself has waited out the period, it times out.
    fx.age_round(ticket.id, 2, 4);
    fx.sweep().await;
    assert_eq!(fx.approval_state(ticket.id).as_deref(), Some("approved"));
}

/// A request closed or cancelled while it waited is never approved by the
/// timeout, and the requester hears nothing about an approval.
#[actix_web::test]
async fn a_closed_request_is_not_approved_by_the_timeout() {
    let fx = Fixture::new();
    for category in [
        WorkflowStateCategory::Done,
        WorkflowStateCategory::Cancelled,
    ] {
        let ticket = fx.waiting_request(&format!("Software {category:?}"), None);
        fx.age_round(ticket.id, 1, 10);
        fx.run(|c| {
            let state = workflow_states::first_in_category(c, category)?;
            Ok(ticket_repo::update_ticket_partial(
                c,
                ticket.id,
                TicketUpdate {
                    workflow_state_id: Some(state.id),
                    updated_at: Some(chrono::Utc::now().naive_utc()),
                    ..Default::default()
                },
                None,
            )?)
        });

        fx.sweep().await;
        assert_eq!(
            fx.approval_state(ticket.id).as_deref(),
            Some("pending"),
            "{category:?}: not approved"
        );
        assert!(
            !fx.run(|c| ticket_approvals::auto_approve(c, ticket.id)),
            "{category:?}: nothing to approve"
        );
        assert_eq!(
            fx.decided_events(ticket.id),
            0,
            "{category:?}: no approval notice"
        );
    }
}
