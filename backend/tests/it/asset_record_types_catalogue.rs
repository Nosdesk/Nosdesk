//! Record types come from a catalogue, not a seed: a new workspace has none;
//! adding a built-in twice gives one row with the catalogue's fields and
//! outcomes, which keeps its slug when renamed; a workspace's own type can't
//! take a catalogue slug; one live type opens with a status; and a change to
//! a type's fields or outcomes checks its records first.

use serde_json::json;

use backend::db::DbConnection;
use backend::models::{NewAsset, OutcomeCategory, RecordOutcome, RecordSource};
use backend::repository::asset_record_types::{
    self as types, NewRecordTypeInput, RecordTypeChanges, RecordTypeError,
};
use backend::repository::asset_records::{self as records, NewRecord, WorkDate};
use backend::repository::assets as asset_repo;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool};

const REF: &str = "test:asset_record_types_catalogue";

fn in_ws<T>(pool: &TestPool, ws: i32, f: impl FnOnce(&mut DbConnection) -> T) -> T {
    run_in_workspace(pool, REF, ws, |c| Ok(f(c))).expect("run in the workspace")
}

fn custom(slug: &str, status: Option<&str>) -> NewRecordTypeInput {
    NewRecordTypeInput {
        slug: slug.to_string(),
        label: "Calibration".to_string(),
        description: None,
        icon: None,
        color: Some("#2563eb".to_string()),
        field_schema: json!({
            "type": "object",
            "properties": {
                "drift": { "type": "number" },
                "lab": { "type": "string" }
            }
        }),
        field_order: vec!["lab".into(), "drift".into(), "nope".into()],
        core_fields: json!({ "reference": "required" }),
        outcomes: vec![RecordOutcome {
            key: "in_tolerance".into(),
            category: OutcomeCategory::Done,
            label: Some("In tolerance".into()),
        }],
        applies_to_kinds: None,
        opens_with_status: status.map(str::to_string),
        sort_order: 50,
        created_by: None,
    }
}

#[test]
fn a_new_workspace_has_no_record_types() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    for ws in [1, seeded.a.workspace_id, seeded.b.workspace_id] {
        let listed = in_ws(&pool, ws, |c| types::list(c, true)).expect("list");
        assert!(listed.is_empty(), "workspace {ws}: {listed:?}");
    }
}

#[test]
fn adding_from_the_catalogue_is_idempotent_and_survives_a_rename() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;

    let first = in_ws(&pool, ws, |c| {
        types::add_from_catalogue(c, "repair", Some(seeded.a.admin_uuid))
    })
    .expect("add repair");
    assert!(first.is_builtin);
    assert_eq!(first.label, None, "the catalogue name translates");
    assert_eq!(first.opens_with_status.as_deref(), Some("in_repair"));
    assert_eq!(
        first.field_order.iter().flatten().collect::<Vec<_>>(),
        ["fault", "offsite", "expected_return", "serial_received"]
    );
    assert_eq!(
        first.field_schema["properties"]["expected_return"],
        json!({ "type": "string", "format": "date" })
    );
    assert_eq!(
        first.outcomes,
        json!([
            { "key": "repaired", "category": "done" },
            { "key": "replaced", "category": "replaced" },
            { "key": "beyond_repair", "category": "not_done" },
            { "key": "no_fault_found", "category": "no_fault" }
        ])
    );

    let renamed = in_ws(&pool, ws, |c| {
        types::update(
            c,
            first.id,
            RecordTypeChanges {
                label: Some(Some("Camera repair".into())),
                ..Default::default()
            },
            false,
        )
    })
    .expect("rename the built-in");
    assert_eq!(renamed.slug, "repair");

    let again = in_ws(&pool, ws, |c| types::add_from_catalogue(c, "repair", None))
        .expect("add repair again");
    assert_eq!(again.id, first.id);
    assert_eq!(again.label.as_deref(), Some("Camera repair"));
    let listed = in_ws(&pool, ws, |c| types::list(c, true)).expect("list");
    assert_eq!(listed.len(), 1, "one row");

    let firmware = in_ws(&pool, ws, |c| {
        types::add_from_catalogue(c, "firmware_update", None)
    })
    .expect("add firmware updates");
    assert_eq!(firmware.core_fields, json!({ "cost": "hidden" }));
    assert_eq!(firmware.opens_with_status, None);
    assert!(matches!(
        in_ws(&pool, ws, |c| types::add_from_catalogue(
            c,
            "calibration",
            None
        )),
        Err(RecordTypeError::NotInCatalogue)
    ));
}

#[test]
fn a_workspaces_own_types_share_status_and_slugs_with_the_catalogue() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;

    assert!(matches!(
        in_ws(&pool, ws, |c| types::create(c, custom("repair", None))),
        Err(RecordTypeError::SlugReserved)
    ));
    let mut odd = custom("calibration", None);
    odd.color = Some("blue".into());
    assert!(matches!(
        in_ws(&pool, ws, |c| types::create(c, odd)),
        Err(RecordTypeError::InvalidColor(_))
    ));
    let mut wordy = custom("calibration", None);
    wordy.label = "L".repeat(256);
    assert!(matches!(
        in_ws(&pool, ws, |c| types::create(c, wordy)),
        Err(RecordTypeError::TooLong {
            field: "label",
            max: 255
        })
    ));
    let own = in_ws(&pool, ws, |c| {
        types::create(c, custom("calibration", Some("in_repair")))
    })
    .expect("a type of the workspace's own");
    assert_eq!(
        own.field_order.iter().flatten().collect::<Vec<_>>(),
        ["lab", "drift"],
        "the order keeps the schema's keys"
    );
    assert!(matches!(
        in_ws(&pool, ws, |c| types::create(
            c,
            custom("calibration_2", Some("in_repair"))
        )),
        Err(RecordTypeError::StatusTaken)
    ));

    let repair =
        in_ws(&pool, ws, |c| types::add_from_catalogue(c, "repair", None)).expect("add repair");
    assert_eq!(
        repair.opens_with_status, None,
        "the workspace's own type already opens with in_repair"
    );

    in_ws(&pool, ws, |c| types::archive(c, own.id)).expect("archive");
    let opened = in_ws(&pool, ws, |c| types::opened_by_status(c, "in_repair")).expect("read");
    assert!(opened.is_none(), "archived types open nothing");
    in_ws(&pool, ws, |c| types::restore(c, own.id)).expect("restore");
}

#[test]
fn a_change_checks_the_types_records_first() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let own = in_ws(&pool, ws, |c| types::create(c, custom("calibration", None))).expect("create");
    let asset = in_ws(&pool, ws, |c| {
        asset_repo::create_device(
            c,
            NewAsset {
                name: "Scale 3".to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: None,
                purchase_date: None,
                asset_tag: None,
                kind: "equipment".to_string(),
                attributes: json!({}),
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
                created_by: None,
            },
        )
    })
    .expect("asset")
    .id;
    let on = chrono::NaiveDate::from_ymd_opt(2024, 4, 2).expect("date");
    let written = in_ws(&pool, ws, |c| {
        records::create(
            c,
            NewRecord {
                asset_id: asset,
                record_type_id: own.id,
                summary: None,
                notes: None,
                occurred: WorkDate::Day(on),
                completed: Some(WorkDate::Day(on)),
                outcome: Some("in_tolerance".into()),
                vendor: None,
                performed_by_uuid: None,
                reference: Some("CAL-88".into()),
                cost: None,
                ticket_id: None,
                replacement_asset_id: None,
                snapshot: None,
                attributes: json!({ "drift": 0.2, "lab": "Metrology Ltd" }),
                source: RecordSource::Import,
                source_ref: Some("legacy-88".into()),
                import_job_id: None,
                rule_id: None,
                lifecycle_event_id: None,
                recorded_by: None,
            },
        )
    })
    .expect("a record");
    assert_eq!(written.record.source, "import");

    let drop_drift = || RecordTypeChanges {
        field_schema: Some(json!({
            "type": "object",
            "properties": { "lab": { "type": "string" } }
        })),
        ..Default::default()
    };
    match in_ws(&pool, ws, |c| types::update(c, own.id, drop_drift(), false)) {
        Err(RecordTypeError::InvalidRecords { count, samples }) => {
            assert_eq!(count, 1);
            assert_eq!(samples[0]["id"], written.record.id);
        }
        other => panic!("expected the preflight to refuse: {other:?}"),
    }
    let forced =
        in_ws(&pool, ws, |c| types::update(c, own.id, drop_drift(), true)).expect("forced");
    assert_eq!(
        forced.field_order.iter().flatten().collect::<Vec<_>>(),
        ["lab"]
    );
    assert!(matches!(
        in_ws(&pool, ws, |c| types::delete(c, own.id)),
        Err(RecordTypeError::InUse { count: 1 })
    ));
}
