//! Ticket and asset locations: agent-only sync, no address in the sync
//! stream or the audit trail, workspace isolation, and export and import.

#![allow(clippy::expect_used)]

use actix_web::http::StatusCode;
use actix_web::test as http_test;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Jsonb, Nullable, Text};
use uuid::Uuid;

use backend::models::{
    GeocodePrecision, LocationInput, LocationOrigin, PlatformRole, SyncAggregate, WorkspaceRole,
};
use backend::repository::locations::{self, LocationError, WriteError};
use backend::repository::ticket_visibility::VisibilityContext;
use backend::services::workspace_import::read_archive;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::sync::visibility::{filter_actions, ActionView, SyncViewer};

use crate::common::{seed_two_workspaces, TestDb, TestPool, TestPooledConn};
use crate::workspace_export_hygiene::call_admin_route;
use crate::workspace_import_apply::{hosted_target, import_into, provision_hosted};

const REF: &str = "test:locations";
const PASSWORD: &str = "locations-password";
/// A street name to look for where no address may be.
const STREET: &str = "12 Quokkaville Parade";

fn as_actor<T>(
    conn: &mut TestPooledConn,
    workspace_id: i32,
    f: impl FnOnce(&mut backend::db::DbConnection) -> Result<T, diesel::result::Error>,
) -> T {
    let actor = ActorContext::system(REF).with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, f).expect("as actor")
}

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

fn new_ticket(conn: &mut backend::db::DbConnection, requester: Option<Uuid>) -> QueryResult<i32> {
    diesel::sql_query(
        "INSERT INTO tickets (title, workflow_state_id, requester_uuid) \
         VALUES ('Fix the router', (SELECT id FROM workflow_states ORDER BY id LIMIT 1), $1) \
         RETURNING id",
    )
    .bind::<Nullable<diesel::sql_types::Uuid>, _>(requester)
    .get_result::<Id>(conn)
    .map(|r| r.id)
}

fn new_asset(conn: &mut backend::db::DbConnection) -> QueryResult<i32> {
    diesel::sql_query(
        "INSERT INTO assets (name, kind, location) VALUES ('Router', 'device', 'Comms cupboard') \
         RETURNING id",
    )
    .get_result::<Id>(conn)
    .map(|r| r.id)
}

fn home() -> LocationInput {
    LocationInput {
        label: Some("Home".into()),
        street: Some(STREET.into()),
        city: Some("Windsor".into()),
        region: Some("VIC".into()),
        postal_code: Some("3181".into()),
        country: Some("AU".into()),
        latitude: Some(-37.856),
        longitude: Some(144.992),
        geocode_precision: Some(GeocodePrecision::House),
        geocode_source: Some("G-NAF".into()),
        geocode_place_confirmed: Some(true),
        origin_type: Some(LocationOrigin::Home),
    }
}

fn write<T>(r: Result<T, WriteError>) -> Result<T, diesel::result::Error> {
    r.map_err(|e| match e {
        WriteError::Db(e) => e,
        WriteError::Invalid(e) => panic!("invalid location: {e}"),
    })
}

#[derive(QueryableByName)]
struct Action {
    #[diesel(sql_type = Text)]
    event_type: String,
    #[diesel(sql_type = Jsonb)]
    data: serde_json::Value,
}

fn actions(conn: &mut TestPooledConn, aggregate: &str) -> Vec<Action> {
    diesel::sql_query(
        "SELECT event_type, data FROM sync_actions \
         WHERE aggregate::text = $1 ORDER BY sync_id",
    )
    .bind::<Text, _>(aggregate)
    .load(conn)
    .expect("sync actions")
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count(conn: &mut TestPooledConn, sql: &str) -> i64 {
    diesel::sql_query(sql)
        .get_result::<Count>(conn)
        .expect("count")
        .n
}

#[test]
fn writes_name_the_row_and_never_the_address() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let ws = seed_two_workspaces(&mut conn).a;

    let (ticket, asset) = as_actor(&mut conn, ws.workspace_id, |c| {
        let ticket = new_ticket(c, Some(ws.member_uuid))?;
        let asset = new_asset(c)?;
        write(locations::set_for_ticket(
            c,
            ticket,
            home(),
            Some(ws.admin_uuid),
        ))?;
        let moved = LocationInput {
            street: Some("14 Quokkaville Parade".into()),
            ..home()
        };
        write(locations::set_for_ticket(
            c,
            ticket,
            moved,
            Some(ws.admin_uuid),
        ))?;
        assert!(locations::clear_for_ticket(c, ticket)?);
        assert!(
            !locations::clear_for_ticket(c, ticket)?,
            "nothing left to clear"
        );
        write(locations::set_for_asset(
            c,
            asset,
            home(),
            Some(ws.admin_uuid),
        ))?;
        Ok((ticket, asset))
    });

    let ticket_actions = actions(&mut conn, "ticket_location");
    assert_eq!(
        ticket_actions
            .iter()
            .map(|a| a.event_type.as_str())
            .collect::<Vec<_>>(),
        [
            "ticket_location.added",
            "ticket_location.updated",
            "ticket_location.removed"
        ]
    );
    for a in &ticket_actions {
        assert_eq!(a.data, serde_json::json!({ "ticket_id": ticket }));
    }
    let asset_actions = actions(&mut conn, "asset_location");
    assert_eq!(asset_actions.len(), 1);
    assert_eq!(asset_actions[0].event_type, "asset_location.added");
    assert_eq!(
        asset_actions[0].data,
        serde_json::json!({ "asset_id": asset })
    );

    // The audit trail says what changed, not what it changed to.
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS n FROM audit_log \
             WHERE table_name IN ('ticket_locations', 'asset_locations')"
        ),
        4
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS n FROM audit_log \
             WHERE table_name IN ('ticket_locations', 'asset_locations') \
               AND (coalesce(before_jsonb::text, '') ILIKE '%quokkaville%' \
                 OR coalesce(after_jsonb::text, '') ILIKE '%quokkaville%' \
                 OR coalesce(before_jsonb::text, '') LIKE '%144.99%' \
                 OR coalesce(after_jsonb::text, '') LIKE '%144.99%')"
        ),
        0,
        "no address or coordinates in the audit trail"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS n FROM audit_log \
             WHERE table_name = 'ticket_locations' AND op = 'U' \
               AND (after_jsonb->>'street_changed')::boolean \
               AND NOT (after_jsonb->>'city_changed')::boolean"
        ),
        1,
        "the update records that the street changed"
    );
}

#[test]
fn a_location_needs_something_to_place() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let ws = seed_two_workspaces(&mut conn).a;
    as_actor(&mut conn, ws.workspace_id, |c| {
        let ticket = new_ticket(c, None)?;
        let half = LocationInput {
            latitude: Some(1.0),
            ..Default::default()
        };
        assert!(matches!(
            locations::set_for_ticket(c, ticket, half, None),
            Err(WriteError::Invalid(LocationError::HalfCoordinates))
        ));
        assert!(matches!(
            locations::set_for_ticket(c, ticket, LocationInput::default(), None),
            Err(WriteError::Invalid(LocationError::Empty))
        ));
        assert!(locations::for_ticket(c, ticket)?.is_none());
        Ok(())
    });
}

#[test]
fn workspaces_cannot_see_each_others_locations() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let seeds = seed_two_workspaces(&mut conn);
    let (a, b) = (seeds.a.workspace_id, seeds.b.workspace_id);

    let (ticket_a, asset_a) = as_actor(&mut conn, a, |c| {
        let t = new_ticket(c, None)?;
        let s = new_asset(c)?;
        write(locations::set_for_ticket(c, t, home(), None))?;
        write(locations::set_for_asset(c, s, home(), None))?;
        Ok((t, s))
    });
    let ticket_b = as_actor(&mut conn, b, |c| {
        let t = new_ticket(c, None)?;
        write(locations::set_for_ticket(c, t, home(), None))?;
        Ok(t)
    });

    #[derive(QueryableByName)]
    struct Seen {
        #[diesel(sql_type = BigInt)]
        tickets: i64,
        #[diesel(sql_type = BigInt)]
        assets: i64,
    }
    let seen = |conn: &mut TestPooledConn, ws: i32| -> (i64, i64) {
        as_actor(conn, ws, |c| {
            diesel::sql_query(
                "SELECT (SELECT count(*) FROM ticket_locations) AS tickets, \
                        (SELECT count(*) FROM asset_locations) AS assets",
            )
            .get_result::<Seen>(c)
            .map(|s| (s.tickets, s.assets))
        })
    };
    assert_eq!(seen(&mut conn, a), (1, 1));
    assert_eq!(seen(&mut conn, b), (1, 0));

    as_actor(&mut conn, b, |c| {
        assert!(locations::for_ticket(c, ticket_a)?.is_none());
        assert!(locations::for_asset(c, asset_a)?.is_none());
        assert!(
            matches!(
                locations::set_for_ticket(c, ticket_a, home(), None),
                Err(WriteError::Db(diesel::result::Error::NotFound))
            ),
            "another workspace's ticket can't be given a location"
        );
        assert!(
            matches!(
                locations::clear_for_ticket(c, ticket_a),
                Err(diesel::result::Error::NotFound)
            ),
            "or lose one"
        );
        assert!(locations::for_ticket(c, ticket_b)?.is_some());
        Ok(())
    });
    as_actor(&mut conn, a, |c| {
        assert!(
            locations::for_ticket(c, ticket_a)?.is_some(),
            "A's location stays"
        );
        Ok(())
    });
}

fn viewer(uuid: Uuid, role: WorkspaceRole) -> SyncViewer {
    SyncViewer {
        ctx: VisibilityContext::new(uuid, PlatformRole::from_db("user"), Some(role)),
        is_admin: false,
    }
}

fn view(aggregate: SyncAggregate, ticket_id: i32) -> ActionView {
    ActionView {
        aggregate: Some(aggregate),
        is_delete: false,
        aggregate_id: Some(ticket_id),
        ticket_id: Some(ticket_id),
        is_internal: None,
        comment_id: None,
        subject_uuid: None,
        bare_id: false,
        addressees: Vec::new(),
        gap_id: None,
    }
}

#[test]
fn locations_reach_agents_and_never_the_requester() {
    crate::common::ensure_test_keyring();
    let db = TestDb::new();
    let mut conn = db.conn();
    let ws = seed_two_workspaces(&mut conn).a;
    let ticket = as_actor(&mut conn, ws.workspace_id, |c| {
        new_ticket(c, Some(ws.member_uuid))
    });

    let items = [
        view(SyncAggregate::Ticket, ticket),
        view(SyncAggregate::TicketLocation, ticket),
        view(SyncAggregate::AssetLocation, ticket),
    ];
    let pick = |v: &ActionView| v.clone();
    let requester = viewer(ws.member_uuid, WorkspaceRole::Member);
    assert_eq!(
        filter_actions(&mut conn, &requester, &items, pick),
        [true, false, false],
        "the requester sees their ticket and none of its locations"
    );
    let agent = viewer(ws.admin_uuid, WorkspaceRole::Agent);
    assert_eq!(
        filter_actions(&mut conn, &agent, &items, pick),
        [true, true, true]
    );
}

#[derive(QueryableByName)]
struct Carried {
    #[diesel(sql_type = Nullable<Text>)]
    ticket_title: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    ticket_street: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    ticket_origin: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    asset_name: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    asset_city: Option<String>,
}

fn carried(pool: &TestPool, ws: i32) -> Carried {
    diesel::sql_query(
        "SELECT t.title AS ticket_title, l.street AS ticket_street, \
                l.origin_type AS ticket_origin, a.name AS asset_name, al.city AS asset_city \
         FROM ticket_locations l JOIN tickets t ON t.id = l.ticket_id \
         CROSS JOIN asset_locations al JOIN assets a ON a.id = al.asset_id \
         WHERE l.workspace_id = $1 AND al.workspace_id = $1",
    )
    .bind::<Integer, _>(ws)
    .get_result(&mut pool.get().expect("conn"))
    .expect("carried locations")
}

#[actix_web::test]
async fn export_and_import_carry_locations() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = {
        let mut conn = pool.get().expect("conn");
        let ws = seed_two_workspaces(&mut conn).a;
        as_actor(&mut conn, ws.workspace_id, |c| {
            let t = new_ticket(c, Some(ws.member_uuid))?;
            let s = new_asset(c)?;
            write(locations::set_for_ticket(c, t, home(), Some(ws.admin_uuid)))?;
            write(locations::set_for_asset(c, s, home(), Some(ws.admin_uuid)))?;
            Ok(())
        });
        ws
    };
    let (status, body) = call_admin_route(
        &pool,
        &ws,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/workspaces/{}/export", ws.workspace_id))
            .set_json(serde_json::json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let target = provision_hosted(&pool);
    let contents = read_archive(&body, Some(PASSWORD)).expect("read");
    import_into(&pool, &contents, &hosted_target(&target)).expect("imports");

    let got = carried(&pool, target.id);
    assert_eq!(got.ticket_title.as_deref(), Some("Fix the router"));
    assert_eq!(got.ticket_street.as_deref(), Some(STREET));
    assert_eq!(got.ticket_origin.as_deref(), Some("home"));
    assert_eq!(got.asset_name.as_deref(), Some("Router"));
    assert_eq!(got.asset_city.as_deref(), Some("Windsor"));
}
