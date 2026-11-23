//! Repairs entered before records become repair records.
//!
//! Runs the migration set against a database seeded, just before the
//! backfill, with lifecycle history the way earlier releases wrote it: repair
//! details in `in_repair` events' metadata, some of it odd, beside moves made
//! by imports and rules and bare moves. Each event with usable details
//! becomes a record dated by the event and completed by the asset's next
//! event; the repair type is added, as the catalogue gives it, only to
//! workspaces with repairs to carry and without it.

#![allow(clippy::expect_used)]

use chrono::{DateTime, NaiveDate, Utc};
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Text};
use diesel_migrations::MigrationHarness;
use serde_json::json;
use uuid::Uuid;

use backend::db::MIGRATIONS;
use backend::models::{AssetRecord, AssetRecordType};
use backend::schema::{asset_record_types, asset_records};
use backend::services::assets::record_catalogue;
use backend::services::custom_fields::schema::validate_attributes;

use crate::migration_backfill_existing_workspace::FreshDb;

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// Run `sql` the way the rows would already exist: the table's user triggers
/// off.
fn existing(conn: &mut PgConnection, table: &str, sql: &str) -> i32 {
    diesel::sql_query(format!("ALTER TABLE {table} DISABLE TRIGGER USER"))
        .execute(conn)
        .expect("disable triggers");
    let id = diesel::sql_query(sql)
        .get_result::<Id>(conn)
        .unwrap_or_else(|e| panic!("seed {table}: {e}"))
        .id;
    diesel::sql_query(format!("ALTER TABLE {table} ENABLE TRIGGER USER"))
        .execute(conn)
        .expect("enable triggers");
    id
}

fn workspace(conn: &mut PgConnection, slug: &str, zone: &str) -> i32 {
    let id = existing(
        conn,
        "workspaces",
        &format!("INSERT INTO workspaces (slug, name) VALUES ('{slug}', '{slug}') RETURNING id"),
    );
    existing(
        conn,
        "site_settings",
        &format!(
            "INSERT INTO site_settings (workspace_id, default_timezone) \
             VALUES ({id}, '{zone}') RETURNING id"
        ),
    );
    id
}

fn asset(conn: &mut PgConnection, workspace: i32, name: &str, status: &str) -> i32 {
    existing(
        conn,
        "assets",
        &format!(
            "INSERT INTO assets (workspace_id, name, kind, status) \
             VALUES ({workspace}, '{name}', 'device', '{status}') RETURNING id"
        ),
    )
}

/// A lifecycle event as an earlier release wrote it.
struct Move<'a> {
    asset: i32,
    from: &'a str,
    to: &'a str,
    metadata: &'a str,
    at: &'a str,
}

/// Write `m` in `workspace`, with its ticket and who made it when given.
fn event(conn: &mut PgConnection, workspace: i32, m: Move, by: Option<(i32, Uuid)>) -> i32 {
    let (ticket, actor) = match by {
        Some((ticket, actor)) => (ticket.to_string(), format!("'{actor}'")),
        None => ("NULL".to_string(), "NULL".to_string()),
    };
    existing(
        conn,
        "asset_lifecycle_events",
        &format!(
            "INSERT INTO asset_lifecycle_events \
                 (workspace_id, asset_id, from_status, to_status, metadata, occurred_at, \
                  ticket_id, actor_uuid) \
             VALUES ({workspace}, {}, '{}', '{}', '{}'::jsonb, '{}', {ticket}, {actor}) \
             RETURNING id",
            m.asset, m.from, m.to, m.metadata, m.at
        ),
    )
}

fn to_repair<'a>(asset: i32, metadata: &'a str, at: &'a str) -> Move<'a> {
    Move {
        asset,
        from: "in_service",
        to: "in_repair",
        metadata,
        at,
    }
}

fn back<'a>(asset: i32, at: &'a str) -> Move<'a> {
    Move {
        asset,
        from: "in_repair",
        to: "in_service",
        metadata: "{}",
        at,
    }
}

struct Seeded {
    sydney: i32,
    bad_zone: i32,
    own_repair: i32,
    calibration: i32,
    nothing_to_carry: i32,
    actor: Uuid,
    ticket: i32,
    /// Repaired and back in service.
    repaired: i32,
    /// Still in repair: an open record.
    still_in_repair: i32,
    /// Left repair by a path that wrote no event.
    reset_without_event: i32,
    /// Blank vendor, an offsite that isn't a flag, a date that isn't one.
    unusable: i32,
    /// Metadata that is a list, not an object.
    listed: i32,
    /// Details from an import and from a rule.
    imported: i32,
    by_rule: i32,
    /// Offsite false, a moment for the expected return, a vendor that is an
    /// object.
    odd_but_usable: i32,
    bad_zone_repair: i32,
    own_repair_event: i32,
    calibration_event: i32,
    own_repair_type: i32,
    calibration_type: i32,
}

fn seed(conn: &mut PgConnection) -> Seeded {
    let actor = Uuid::new_v4();
    existing(
        conn,
        "users",
        &format!("INSERT INTO users (uuid, name) VALUES ('{actor}', 'Agent') RETURNING 0 AS id"),
    );
    let sydney = workspace(conn, "repairs-sydney", "Australia/Sydney");
    let state = existing(
        conn,
        "workflow_states",
        &format!(
            "INSERT INTO workflow_states (workspace_id, name, category, color, position, is_default) \
             VALUES ({sydney}, 'Open', 'backlog', 'gray', 0, true) RETURNING id"
        ),
    );
    let ticket = existing(
        conn,
        "tickets",
        &format!(
            "INSERT INTO tickets (workspace_id, title, workflow_state_id, number) \
             VALUES ({sydney}, 'Camera won''t focus', {state}, 1) RETURNING id"
        ),
    );

    let camera = asset(conn, sydney, "Camera", "in_service");
    let repaired = event(
        conn,
        sydney,
        to_repair(
            camera,
            r#"{"vendor": " Camera Clinic ", "rma_number": "RMA-4471", "offsite": true, "expected_return": "2025-03-10"}"#,
            "2025-03-01T22:30:00Z",
        ),
        Some((ticket, actor)),
    );
    event(conn, sydney, back(camera, "2025-03-09T01:00:00Z"), None);

    let tripod = asset(conn, sydney, "Tripod", "in_repair");
    let still_in_repair = event(
        conn,
        sydney,
        to_repair(
            tripod,
            r#"{"vendor": "Tripod Works"}"#,
            "2025-04-01T00:00:00Z",
        ),
        None,
    );

    let light = asset(conn, sydney, "Light", "in_stock");
    let reset_without_event = event(
        conn,
        sydney,
        to_repair(light, r#"{"rma_number": 12345}"#, "2025-05-01T00:00:00Z"),
        None,
    );

    let odd = asset(conn, sydney, "Odd", "in_service");
    let odd_move = |conn: &mut PgConnection, metadata: &str, at: &str, out: &str| {
        let id = event(conn, sydney, to_repair(odd, metadata, at), None);
        event(conn, sydney, back(odd, out), None);
        id
    };
    let unusable = odd_move(
        conn,
        r#"{"vendor": "  ", "offsite": "yes", "expected_return": "2026-02-30"}"#,
        "2025-06-01T00:00:00Z",
        "2025-06-02T00:00:00Z",
    );
    let listed = odd_move(
        conn,
        r#"["vendor", "rma_number"]"#,
        "2025-06-03T00:00:00Z",
        "2025-06-04T00:00:00Z",
    );
    let imported = odd_move(
        conn,
        r#"{"vendor": "Importer", "import_job_id": "6b0c3b5e-9a51-4a43-8d1e-9c1f3d7a2b10"}"#,
        "2025-06-05T00:00:00Z",
        "2025-06-06T00:00:00Z",
    );
    let by_rule = odd_move(
        conn,
        r#"{"vendor": "Rules", "rule_id": 3}"#,
        "2025-06-07T00:00:00Z",
        "2025-06-08T00:00:00Z",
    );
    let odd_but_usable = odd_move(
        conn,
        r#"{"vendor": {"name": "x"}, "offsite": false, "expected_return": "2025-07-01T00:00:00Z", "rma_number": "R-9"}"#,
        "2025-06-09T00:00:00Z",
        "2025-06-10T00:00:00Z",
    );

    // A timezone Postgres doesn't know reads as UTC.
    let bad_zone = workspace(conn, "repairs-bad-zone", "Not/AZone");
    let drone = asset(conn, bad_zone, "Drone", "in_service");
    let bad_zone_repair = event(
        conn,
        bad_zone,
        to_repair(
            drone,
            r#"{"vendor": "Drone Doctor"}"#,
            "2025-01-01T23:30:00Z",
        ),
        None,
    );
    event(conn, bad_zone, back(drone, "2025-01-03T09:00:00Z"), None);

    // A workspace that added the repair type already and changed its fields.
    let own_repair = workspace(conn, "repairs-own-type", "UTC");
    let own_repair_type = existing(
        conn,
        "asset_record_types",
        &format!(
            "INSERT INTO asset_record_types \
                 (workspace_id, slug, label, field_schema, field_order, outcomes, is_builtin, \
                  opens_with_status) \
             VALUES ({own_repair}, 'repair', 'Fixes', \
                     '{{\"type\": \"object\", \"properties\": {{\"offsite\": {{\"type\": \"boolean\"}}, \
                       \"bay\": {{\"type\": \"string\"}}}}}}', \
                     ARRAY['offsite', 'bay'], '[]', true, 'in_repair') RETURNING id"
        ),
    );
    let mixer = asset(conn, own_repair, "Mixer", "in_service");
    let own_repair_event = event(
        conn,
        own_repair,
        to_repair(
            mixer,
            r#"{"offsite": true, "expected_return": "2025-08-01"}"#,
            "2025-07-20T00:00:00Z",
        ),
        None,
    );
    event(conn, own_repair, back(mixer, "2025-07-30T00:00:00Z"), None);

    // A workspace whose own type opens with in_repair.
    let calibration = workspace(conn, "repairs-calibration", "UTC");
    let calibration_type = existing(
        conn,
        "asset_record_types",
        &format!(
            "INSERT INTO asset_record_types \
                 (workspace_id, slug, label, outcomes, opens_with_status) \
             VALUES ({calibration}, 'calibration', 'Calibration', '[]', 'in_repair') RETURNING id"
        ),
    );
    let meter = asset(conn, calibration, "Meter", "in_service");
    let calibration_event = event(
        conn,
        calibration,
        to_repair(meter, r#"{"vendor": "Labs"}"#, "2025-09-01T00:00:00Z"),
        None,
    );
    event(conn, calibration, back(meter, "2025-09-02T00:00:00Z"), None);

    // Bare, imported and rule moves only: nothing to carry, no type.
    let nothing_to_carry = workspace(conn, "repairs-nothing", "UTC");
    let printer = asset(conn, nothing_to_carry, "Printer", "in_service");
    for (metadata, at, out) in [
        ("{}", "2025-02-01T00:00:00Z", "2025-02-02T00:00:00Z"),
        (
            r#"{"vendor": "Importer", "import_job_id": "6b0c3b5e-9a51-4a43-8d1e-9c1f3d7a2b10"}"#,
            "2025-02-03T00:00:00Z",
            "2025-02-04T00:00:00Z",
        ),
        (
            r#"{"vendor": "Rules", "rule_id": 3}"#,
            "2025-02-05T00:00:00Z",
            "2025-02-06T00:00:00Z",
        ),
    ] {
        event(
            conn,
            nothing_to_carry,
            to_repair(printer, metadata, at),
            None,
        );
        event(conn, nothing_to_carry, back(printer, out), None);
    }

    Seeded {
        sydney,
        bad_zone,
        own_repair,
        calibration,
        nothing_to_carry,
        actor,
        ticket,
        repaired,
        still_in_repair,
        reset_without_event,
        unusable,
        listed,
        imported,
        by_rule,
        odd_but_usable,
        bad_zone_repair,
        own_repair_event,
        calibration_event,
        own_repair_type,
        calibration_type,
    }
}

fn upgrade() -> (FreshDb, PgConnection, Seeded) {
    let db = FreshDb::new();
    let mut conn = backend::db::establish(&db.url).expect("connect fresh db");
    diesel::sql_query(
        "CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (\
         version VARCHAR(50) PRIMARY KEY NOT NULL, \
         run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(&mut conn)
    .expect("create migrations table");
    let mut pending = conn
        .pending_migrations(MIGRATIONS)
        .expect("list pending migrations");
    pending.sort_by_key(|m| m.name().to_string());
    let mut seeded = None;
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("_asset_repair_record_backfill") {
            seeded = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    (db, conn, seeded.expect("the migration ran"))
}

/// The backfill migration's `file`, found by name so a rename can't strand
/// the test.
fn backfill_sql(file: &str) -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let migration = std::fs::read_dir(&dir)
        .expect("read the migrations")
        .filter_map(Result::ok)
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .ends_with("_asset_repair_record_backfill")
        })
        .expect("the backfill migration");
    std::fs::read_to_string(migration.path().join(file)).expect("read the migration")
}

fn record_for(conn: &mut PgConnection, event: i32) -> Option<AssetRecord> {
    asset_records::table
        .filter(asset_records::lifecycle_event_id.eq(event))
        .first(conn)
        .optional()
        .expect("read records")
}

fn types_of(conn: &mut PgConnection, workspace: i32) -> Vec<AssetRecordType> {
    asset_record_types::table
        .filter(asset_record_types::workspace_id.eq(workspace))
        .order(asset_record_types::id)
        .load(conn)
        .expect("read types")
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("a date")
}

fn at(s: &str) -> DateTime<Utc> {
    s.parse().expect("a moment")
}

#[test]
fn repair_details_become_records_and_the_rest_stay_moves() {
    let (_db, mut conn, s) = upgrade();
    let c = &mut conn;

    // Dated by the event in the workspace's zone, completed by the next one,
    // entered by whoever made the move.
    let r = record_for(c, s.repaired).expect("a record");
    assert_eq!(r.workspace_id, s.sydney);
    assert_eq!(r.occurred_at, Some(at("2025-03-01T22:30:00Z")));
    assert_eq!(r.occurred_on, day(2025, 3, 2), "the day in Sydney");
    assert_eq!(r.completed_at, Some(at("2025-03-09T01:00:00Z")));
    assert_eq!(r.completed_on, Some(day(2025, 3, 9)));
    assert_eq!(r.vendor.as_deref(), Some("Camera Clinic"));
    assert_eq!(r.reference.as_deref(), Some("RMA-4471"));
    assert_eq!(
        r.attributes,
        json!({ "offsite": true, "expected_return": "2025-03-10" })
    );
    assert_eq!(r.ticket_id, Some(s.ticket));
    assert_eq!(r.outcome, None);
    assert_eq!(r.source, "backfill");
    assert_eq!(r.recorded_at, at("2025-03-01T22:30:00Z"));
    assert_eq!(r.recorded_by, Some(s.actor));
    assert_eq!(r.snapshot_source, "none");
    assert_eq!(r.version, 1);

    // No exit: open, whether the asset is still in repair or not.
    let open = record_for(c, s.still_in_repair).expect("a record");
    assert_eq!((open.completed_on, open.completed_at), (None, None));
    assert_eq!(open.vendor.as_deref(), Some("Tripod Works"));
    let reset = record_for(c, s.reset_without_event).expect("a record");
    assert_eq!(reset.completed_on, None);
    assert_eq!(
        reset.reference.as_deref(),
        Some("12345"),
        "a number reads as text"
    );

    // Nothing usable, a list, an import's and a rule's moves: no record.
    for (event, why) in [
        (s.unusable, "nothing usable"),
        (s.listed, "a list"),
        (s.imported, "an import's move"),
        (s.by_rule, "a rule's move"),
    ] {
        assert!(record_for(c, event).is_none(), "{why}");
    }

    let odd = record_for(c, s.odd_but_usable).expect("a record");
    assert_eq!(odd.vendor, None, "an object isn't a vendor");
    assert_eq!(odd.reference.as_deref(), Some("R-9"));
    assert_eq!(
        odd.attributes,
        json!({ "offsite": false, "expected_return": "2025-07-01" })
    );

    let zone = record_for(c, s.bad_zone_repair).expect("a record");
    assert_eq!(zone.occurred_on, day(2025, 1, 1), "UTC");
    assert_eq!(zone.completed_on, Some(day(2025, 1, 3)));

    // The workspace's own repair type is used, with only the fields it has.
    let own = record_for(c, s.own_repair_event).expect("a record");
    assert_eq!(own.record_type_id, s.own_repair_type);
    assert_eq!(own.attributes, json!({ "offsite": true }));

    let backfilled: i64 = asset_records::table
        .filter(asset_records::source.eq("backfill"))
        .count()
        .get_result(c)
        .expect("count");
    assert_eq!(backfilled, 7);

    // Every record fits its type, so the record form can edit and complete
    // it.
    let rows: Vec<AssetRecord> = asset_records::table.load(c).expect("records");
    for row in rows {
        let rtype: AssetRecordType = asset_record_types::table
            .find(row.record_type_id)
            .first(c)
            .expect("its type");
        validate_attributes(&rtype.field_schema, &row.attributes)
            .unwrap_or_else(|e| panic!("record {}: {e}", row.id));
    }
}

#[test]
fn the_repair_type_is_added_where_there_are_repairs_to_carry() {
    let (_db, mut conn, s) = upgrade();
    let c = &mut conn;
    let entry = record_catalogue::entry("repair").expect("the catalogue's repair");

    // As the catalogue gives it.
    let types = types_of(c, s.sydney);
    assert_eq!(types.len(), 1);
    let repair = &types[0];
    assert_eq!(repair.slug, "repair");
    assert_eq!(repair.label, None);
    assert!(repair.is_builtin);
    assert_eq!(repair.icon.as_deref(), Some(entry.icon));
    assert_eq!(repair.sort_order, entry.sort_order);
    assert_eq!(repair.field_schema, entry.field_schema());
    assert_eq!(
        repair
            .field_order
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>(),
        entry.field_order()
    );
    assert_eq!(repair.core_fields, entry.core_fields());
    assert_eq!(repair.outcomes, entry.outcomes());
    assert_eq!(repair.opens_with_status.as_deref(), Some("in_repair"));
    assert_eq!(repair.applies_to_kinds, None);
    assert_eq!(repair.archived_at, None);
    assert_eq!(
        record_for(c, s.repaired).map(|r| r.record_type_id),
        Some(repair.id)
    );

    assert_eq!(types_of(c, s.bad_zone).len(), 1);
    assert!(
        types_of(c, s.nothing_to_carry).is_empty(),
        "nothing to carry"
    );

    let own = types_of(c, s.own_repair);
    assert_eq!(own.len(), 1, "the type it had");
    assert_eq!(own[0].label.as_deref(), Some("Fixes"));

    // Added beside a type already opening with in_repair, without the
    // status.
    let types = types_of(c, s.calibration);
    assert_eq!(types.len(), 2);
    assert_eq!(types[0].id, s.calibration_type);
    assert_eq!(types[0].opens_with_status.as_deref(), Some("in_repair"));
    assert_eq!(types[1].slug, "repair");
    assert_eq!(types[1].opens_with_status, None);
    assert_eq!(
        record_for(c, s.calibration_event).map(|r| r.record_type_id),
        Some(types[1].id)
    );
}

#[test]
fn the_backfill_writes_no_audit_rows_and_leaves_the_triggers_on() {
    let (_db, mut conn, _s) = upgrade();
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = BigInt)]
        n: i64,
    }
    let audited = diesel::sql_query(
        "SELECT count(*) AS n FROM audit_log \
         WHERE table_name IN ('asset_records', 'asset_record_types')",
    )
    .get_result::<Count>(&mut conn)
    .expect("count audit rows")
    .n;
    assert_eq!(audited, 0);

    #[derive(QueryableByName)]
    struct Trigger {
        #[diesel(sql_type = Text)]
        tgname: String,
        #[diesel(sql_type = Text)]
        enabled: String,
    }
    let triggers = diesel::sql_query(
        "SELECT tgname::text, tgenabled::text AS enabled FROM pg_trigger \
         WHERE tgname IN ('tr_audit_asset_records', 'tr_audit_asset_record_types') \
         ORDER BY tgname",
    )
    .load::<Trigger>(&mut conn)
    .expect("read triggers");
    assert_eq!(triggers.len(), 2);
    for t in triggers {
        assert_eq!(t.enabled, "O", "{} is on", t.tgname);
    }

    // Reverted and run again, and run over its own records, it carries the
    // same repairs once.
    let count = |conn: &mut PgConnection| -> i64 {
        asset_records::table
            .count()
            .get_result(conn)
            .expect("count records")
    };
    let before = count(&mut conn);
    conn.batch_execute(&backfill_sql("down.sql"))
        .expect("revert the backfill");
    assert_eq!(count(&mut conn), 0);
    conn.batch_execute(&backfill_sql("up.sql"))
        .expect("run the backfill again");
    assert_eq!(count(&mut conn), before);
    conn.batch_execute(&backfill_sql("up.sql"))
        .expect("run the backfill over its own records");
    assert_eq!(count(&mut conn), before, "no repair carried twice");
}
