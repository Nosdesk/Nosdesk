//! The SLA breach sweep says how many people it told. A breach is notified
//! to the ticket's assignee and watchers; a ticket with neither is told to the
//! workspace's admins instead. The sweep's info line counts both.

#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use diesel::prelude::*;
use tokio::sync::RwLock;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;
use uuid::Uuid;

use backend::models::NewTicket;
use backend::services::notifications::NotificationService;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

/// One captured event: its message and its fields, rendered.
#[derive(Debug, Default)]
struct Captured {
    message: String,
    fields: HashMap<String, String>,
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Captured>>>);

struct Grab<'a>(&'a mut Captured);
impl Visit for Grab<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let rendered = format!("{value:?}");
        if field.name() == "message" {
            self.0.message = rendered;
        } else {
            self.0.fields.insert(field.name().to_string(), rendered);
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut captured = Captured::default();
        event.record(&mut Grab(&mut captured));
        self.0.lock().expect("capture").push(captured);
    }
}

/// An open ticket in `workspace` whose response target passed an hour ago.
fn overdue_ticket(
    conn: &mut backend::db::DbConnection,
    workspace: i32,
    title: &str,
    assignee: Option<Uuid>,
) -> i32 {
    use backend::schema::tickets;
    let actor = ActorContext::system("test:sla_breach_sweep").with_workspace(workspace);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        let state = backend::repository::workflow_states::default_state(c)?;
        let past = chrono::Utc::now().naive_utc() - chrono::Duration::hours(1);
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: title.to_string(),
                workflow_state_id: state.id,
                assignee_uuid: assignee,
                ..Default::default()
            })
            .returning(tickets::id)
            .get_result::<i32>(c)
            .and_then(|id| {
                diesel::update(tickets::table.find(id))
                    .set(tickets::sla_response_target_at.eq(Some(past)))
                    .execute(c)
                    .map(|_| id)
            })
    })
    .expect("overdue ticket")
}

#[actix_web::test]
async fn the_breach_sweep_counts_who_it_told_and_breaches_with_no_one_to_tell() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let a = ws.a.workspace_id;
    overdue_ticket(&mut conn, a, "Printer on fire", Some(ws.a.admin_uuid));
    // Created in the app with no assignee, and no one has commented yet.
    overdue_ticket(&mut conn, a, "VPN down", None);
    drop(conn);

    let capture = Capture::default();
    let events = capture.0.clone();
    let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(capture));
    let notifications = Arc::new(NotificationService::new(
        pool.clone(),
        Arc::new(RwLock::new(HashMap::new())),
    ));
    backend::services::scheduled_jobs::detect_sla_breaches(pool.clone(), notifications)
        .await
        .expect("sweep");

    let events = events.lock().expect("events");
    let swept = events
        .iter()
        .find(|e| e.message == "scheduler: SLA breach detection swept")
        .expect("the sweep's info line");
    let field = |name: &str| swept.fields.get(name).map(String::as_str);
    assert_eq!(field("processed"), Some("2"), "{swept:?}");
    assert_eq!(field("notified"), Some("1"), "{swept:?}");
    assert_eq!(field("notify_failed"), Some("0"), "{swept:?}");
    assert_eq!(field("no_recipient"), Some("1"), "{swept:?}");
}

/// A member of `workspace` with `role`, added by its admin.
fn member_with_role(
    conn: &mut backend::db::DbConnection,
    workspace: i32,
    by: Uuid,
    name: &str,
    role: &str,
) -> Uuid {
    use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
    let user = crate::common::insert_plain_user(conn, name);
    let actor = ActorContext::user(by, None).with_workspace(workspace);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        add_membership(c, workspace, user, role, SeatWriteAuthority::ControlPlane)
    })
    .expect("add member");
    user
}

/// Who holds an SLA breach notification in `workspace`.
fn breach_notified(conn: &mut backend::db::DbConnection, workspace: i32) -> Vec<Uuid> {
    use backend::schema::{notification_types, notifications};
    let actor = ActorContext::system("test:sla_breach_sweep").with_workspace(workspace);
    let mut held = with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        notifications::table
            .inner_join(notification_types::table)
            .filter(notification_types::code.eq("sla_breached"))
            .filter(notifications::workspace_id.eq(workspace))
            .select(notifications::user_uuid)
            .load::<Uuid>(c)
    })
    .expect("notifications");
    held.sort();
    held
}

#[actix_web::test]
async fn a_breach_with_no_one_to_tell_notifies_the_workspaces_admins_once() {
    use backend::services::notifications::channels::in_app::InAppChannel;
    use backend::services::notifications::types::{
        NotificationChannel, NotificationFrequency, NotificationTypeCode,
    };

    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let a = ws.a.workspace_id;
    let admin = ws.a.admin_uuid;
    let owner = member_with_role(&mut conn, a, admin, "A Owner", "owner");
    let opted_out = member_with_role(&mut conn, a, admin, "A Quiet Admin", "admin");
    member_with_role(&mut conn, a, admin, "A Agent", "agent");
    // Created in the app with no assignee, and no one has commented yet.
    overdue_ticket(&mut conn, a, "VPN down", None);

    let notifications = Arc::new(NotificationService::new(
        pool.clone(),
        Arc::new(RwLock::new(HashMap::new())),
    ));
    notifications.register_channel(Arc::new(InAppChannel::new()));
    notifications
        .preferences()
        .set_preference(
            &opted_out,
            &NotificationTypeCode::SlaBreached,
            NotificationChannel::InApp,
            NotificationFrequency::Off,
        )
        .await
        .expect("opt out");

    let capture = Capture::default();
    let events = capture.0.clone();
    let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(capture));
    backend::services::scheduled_jobs::detect_sla_breaches(pool.clone(), notifications.clone())
        .await
        .expect("sweep");

    let mut expected = vec![admin, owner];
    expected.sort();
    assert_eq!(
        breach_notified(&mut conn, a),
        expected,
        "the owner and admins are told, except one who turned it off; agents and members aren't"
    );
    assert!(
        breach_notified(&mut conn, ws.b.workspace_id).is_empty(),
        "another workspace's admins aren't told"
    );
    {
        let events = events.lock().expect("events");
        let swept = events
            .iter()
            .find(|e| e.message == "scheduler: SLA breach detection swept")
            .expect("the sweep's info line");
        let field = |name: &str| swept.fields.get(name).map(String::as_str);
        assert_eq!(field("no_recipient"), Some("1"), "{swept:?}");
        // Counted like `notified`: notices handed to the notification service,
        // which then applies each admin's preferences.
        assert_eq!(field("notified"), Some("3"), "{swept:?}");
        assert_eq!(field("admins_notified"), Some("3"), "{swept:?}");
    }

    backend::services::scheduled_jobs::detect_sla_breaches(pool.clone(), notifications)
        .await
        .expect("second sweep");
    assert_eq!(
        breach_notified(&mut conn, a),
        expected,
        "a later sweep doesn't tell them again"
    );
}
