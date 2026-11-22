//! Asset records through the repository: a record never moves its asset's
//! status, keeps when the work was done apart from when it was entered,
//! takes the holder and location of the day of the work, holds to its
//! type's outcomes, refuses impossible dates and costs, guards concurrent
//! edits, stays in its workspace, and keeps its entry clock from the app
//! role.

use chrono::{Duration, NaiveDate, TimeZone, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Integer, Text, Uuid as SqlUuid};
use serde_json::json;
use uuid::Uuid;

use backend::db::DbConnection;
use backend::models::{AssetRecordType, NewAsset, RecordSource};
use backend::repository::asset_records::{
    self as records, Cost, NewRecord, RecordChanges, RecordError, RecordWarning, Snapshot, WorkDate,
};
use backend::repository::{asset_media, asset_record_types, assets as asset_repo, workspaces};
use backend::schema::{asset_lifecycle_events, asset_media as media_t, asset_records, assets};
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_bypass_context};

use crate::common::{self, TestPool};

const REF: &str = "test:asset_records_repo";

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = Integer)]
    id: i32,
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("a date")
}

/// Run `f` as the app role pinned to workspace `ws`.
fn in_ws<T>(pool: &TestPool, ws: i32, f: impl FnOnce(&mut DbConnection) -> T) -> T {
    run_in_workspace(pool, REF, ws, |c| Ok(f(c))).expect("run in the workspace")
}

fn new_asset(pool: &TestPool, ws: i32, name: &str) -> i32 {
    in_ws(pool, ws, |c| {
        asset_repo::create_device(
            c,
            NewAsset {
                name: name.to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: Some("Studio A".to_string()),
                notes: None,
                primary_user_uuid: None,
                purchase_date: Some(day(2020, 1, 15)),
                asset_tag: None,
                kind: "device".to_string(),
                attributes: json!({}),
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
                created_by: None,
            },
        )
    })
    .expect("create an asset")
    .id
}

fn repair_type(pool: &TestPool, ws: i32) -> AssetRecordType {
    in_ws(pool, ws, |c| {
        asset_record_types::add_from_catalogue(c, "repair", None)
    })
    .expect("add the repair type")
}

/// A completed repair on `asset`, done on `on`, as `who` entered it.
fn record(asset: i32, type_id: i32, on: NaiveDate, who: Option<Uuid>) -> NewRecord {
    NewRecord {
        asset_id: asset,
        record_type_id: type_id,
        summary: Some("Lens mount replaced".to_string()),
        notes: None,
        occurred: WorkDate::Day(on),
        completed: Some(WorkDate::Day(on)),
        outcome: Some("repaired".to_string()),
        vendor: Some("Camera Clinic".to_string()),
        performed_by_uuid: None,
        reference: Some("RMA-4471".to_string()),
        cost: Some(Cost {
            minor: 125_050,
            currency: "AUD".to_string(),
        }),
        ticket_id: None,
        replacement_asset_id: None,
        snapshot: None,
        attributes: json!({ "fault": "Lens mount loose", "offsite": true }),
        source: RecordSource::Manual,
        source_ref: None,
        import_job_id: None,
        rule_id: None,
        lifecycle_event_id: None,
        recorded_by: who,
    }
}

fn status_of(pool: &TestPool, ws: i32, asset: i32) -> (String, chrono::NaiveDateTime, i64) {
    in_ws(pool, ws, |c| {
        let (status, updated_at) = assets::table
            .find(asset)
            .select((assets::status, assets::updated_at))
            .first::<(String, chrono::NaiveDateTime)>(c)?;
        let events = asset_lifecycle_events::table
            .filter(asset_lifecycle_events::asset_id.eq(asset))
            .count()
            .get_result::<i64>(c)?;
        QueryResult::Ok((status, updated_at, events))
    })
    .expect("read the asset")
}

fn refused(result: Result<records::RecordWrite, RecordError>) -> RecordError {
    match result {
        Ok(w) => panic!("expected a refusal, got record {}", w.record.id),
        Err(e) => e,
    }
}

/// The headline promise: a 2023 repair on an asset in service leaves it in
/// service, and so does an open repair dated today.
#[test]
fn records_never_change_the_assets_status() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let asset = new_asset(&pool, ws, "Camera 14");
    let repair = repair_type(&pool, ws);
    let before = status_of(&pool, ws, asset);
    assert_eq!(before.0, "in_service");

    in_ws(&pool, ws, |c| {
        records::create(c, record(asset, repair.id, day(2023, 5, 3), None))
    })
    .expect("a 2023 repair");
    let today = Utc::now().date_naive();
    let mut open = record(asset, repair.id, today, None);
    open.completed = None;
    open.outcome = None;
    in_ws(&pool, ws, |c| records::create(c, open)).expect("an open repair today");

    assert_eq!(
        status_of(&pool, ws, asset),
        before,
        "status, updated_at and the lifecycle log are untouched"
    );
}

/// The day of the work is apart from when it was entered and by whom; a
/// moment's day is the workspace's.
#[test]
fn work_is_dated_apart_from_its_entry() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let asset = new_asset(&pool, ws, "Camera 15");
    let repair = repair_type(&pool, ws);
    in_ws(&pool, ws, |c| {
        backend::repository::site_settings::get_site_settings(c)?;
        diesel::sql_query("UPDATE site_settings SET default_timezone = 'Australia/Sydney'")
            .execute(c)
    })
    .expect("set the timezone");

    let mut input = record(asset, repair.id, day(2023, 5, 3), Some(seeded.a.admin_uuid));
    // 20:00 UTC on 2 May is the morning of 3 May in Sydney.
    let at = Utc
        .with_ymd_and_hms(2023, 5, 2, 20, 0, 0)
        .single()
        .expect("a moment");
    input.occurred = WorkDate::At(at);
    input.completed = Some(WorkDate::At(at + Duration::hours(3)));
    let written = in_ws(&pool, ws, |c| records::create(c, input)).expect("create");
    let r = &written.record;
    assert_eq!(r.occurred_on, day(2023, 5, 3));
    assert_eq!(r.occurred_at, Some(at));
    assert_eq!(r.completed_on, Some(day(2023, 5, 3)));
    assert_eq!(r.recorded_by, Some(seeded.a.admin_uuid));
    assert!(Utc::now() - r.recorded_at < Duration::minutes(5));
    assert_eq!((r.version, r.source.as_str()), (1, "manual"));
    assert_eq!(
        (r.cost_minor, r.cost_currency.as_deref()),
        (Some(125_050), Some("AUD"))
    );
    assert!(
        written.warnings.contains(&RecordWarning::HolderUnknown),
        "nothing tells who had it in 2023: {:?}",
        written.warnings
    );
}

/// Holder and location: entered values win, then a loan covering the day,
/// then the asset as it is for work done today, else nothing.
#[test]
fn the_holder_is_the_one_on_the_day_of_the_work() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let asset = new_asset(&pool, ws, "Camera 16");
    let repair = repair_type(&pool, ws);
    in_ws(&pool, ws, |c| {
        diesel::sql_query(
            "INSERT INTO asset_loans (asset_id, borrower_user_uuid, status_before, loaned_at, returned_at) \
             VALUES ($1, $2, 'in_service', '2024-03-01T09:00:00Z', '2024-03-20T09:00:00Z')",
        )
        .bind::<Integer, _>(asset)
        .bind::<SqlUuid, _>(seeded.a.member_uuid)
        .execute(c)
    })
    .expect("a past loan");
    in_ws(&pool, ws, |c| {
        diesel::sql_query("UPDATE assets SET primary_user_uuid = $2 WHERE id = $1")
            .bind::<Integer, _>(asset)
            .bind::<SqlUuid, _>(seeded.a.admin_uuid)
            .execute(c)
    })
    .expect("a current holder");

    let snap = |on: NaiveDate, snapshot: Option<Snapshot>| {
        let mut input = record(asset, repair.id, on, None);
        input.snapshot = snapshot;
        let w = in_ws(&pool, ws, |c| records::create(c, input)).expect("create");
        (
            w.record.holder_uuid,
            w.record.location,
            w.record.snapshot_source,
        )
    };

    assert_eq!(
        snap(
            day(2024, 3, 10),
            Some(Snapshot {
                holder_uuid: None,
                location: Some("Van 2".to_string())
            })
        ),
        (None, Some("Van 2".to_string()), "entered".to_string()),
        "entered values win, blanks included"
    );
    assert_eq!(
        snap(day(2024, 3, 10), None),
        (Some(seeded.a.member_uuid), None, "loan".to_string()),
        "the borrower on the day"
    );
    assert_eq!(
        snap(Utc::now().date_naive(), None),
        (
            Some(seeded.a.admin_uuid),
            Some("Studio A".to_string()),
            "current".to_string()
        ),
        "today: the asset as it is"
    );
    assert_eq!(
        snap(day(2022, 6, 1), None),
        (None, None, "none".to_string()),
        "long ago, with no loan: nothing"
    );
}

/// Outcomes come from the type, only once the work is complete, and only a
/// replaced outcome names a replacement, which is another asset.
#[test]
fn outcomes_follow_the_type() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let asset = new_asset(&pool, ws, "Camera 17");
    let spare = new_asset(&pool, ws, "Camera 18");
    let repair = repair_type(&pool, ws);
    let on = day(2024, 2, 1);
    let try_create = |edit: &dyn Fn(&mut NewRecord)| {
        let mut input = record(asset, repair.id, on, None);
        edit(&mut input);
        in_ws(&pool, ws, |c| records::create(c, input))
    };

    assert!(matches!(
        refused(try_create(&|r| r.completed = None)),
        RecordError::OutcomeWhileOpen
    ));
    assert!(matches!(
        refused(try_create(&|r| r.outcome = Some("passed".into()))),
        RecordError::UnknownOutcome { .. }
    ));
    assert!(matches!(
        refused(try_create(&|r| r.replacement_asset_id = Some(spare))),
        RecordError::ReplacementNeedsReplacedOutcome
    ));
    assert!(matches!(
        refused(try_create(&|r| {
            r.outcome = Some("replaced".into());
            r.replacement_asset_id = Some(asset);
        })),
        RecordError::ReplacementIsSelf
    ));
    let replaced = try_create(&|r| {
        r.outcome = Some("replaced".into());
        r.replacement_asset_id = Some(spare);
    })
    .expect("a replaced outcome names the replacement");
    assert_eq!(replaced.record.replacement_asset_id, Some(spare));
    let open = try_create(&|r| {
        r.completed = None;
        r.outcome = None;
    })
    .expect("open work has no outcome");
    assert_eq!(open.record.outcome, None);
    assert!(matches!(
        refused(try_create(&|r| r.attributes = json!({ "offsite": "yes" }))),
        RecordError::Attributes(_)
    ));
}

/// Dates run from 1900 to tomorrow, completion after the work; costs are in
/// a known currency.
#[test]
fn dates_and_costs_are_checked() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let asset = new_asset(&pool, ws, "Camera 19");
    let repair = repair_type(&pool, ws);
    let attempt = |on: NaiveDate, edit: &dyn Fn(&mut NewRecord)| {
        let mut input = record(asset, repair.id, on, None);
        edit(&mut input);
        in_ws(&pool, ws, |c| records::create(c, input))
    };
    let today = Utc::now().date_naive();

    assert!(matches!(
        refused(attempt(day(1899, 12, 31), &|_| {})),
        RecordError::TooEarly
    ));
    assert!(matches!(
        refused(attempt(today + Duration::days(3), &|_| {})),
        RecordError::InFuture
    ));
    assert!(matches!(
        refused(attempt(day(2024, 2, 10), &|r| r.completed =
            Some(WorkDate::Day(day(2024, 2, 9))))),
        RecordError::CompletedBeforeOccurred
    ));
    assert!(matches!(
        refused(attempt(day(2024, 2, 10), &|r| r.cost = Some(Cost {
            minor: 100,
            currency: "XYZ".into()
        }))),
        RecordError::UnknownCurrency { .. }
    ));

    let early = attempt(day(2019, 6, 1), &|r| {
        r.completed = Some(WorkDate::Day(day(2025, 6, 2)));
    })
    .expect("before the purchase date is allowed");
    assert!(early.warnings.contains(&RecordWarning::BeforePurchase {
        purchase_date: day(2020, 1, 15)
    }));
    assert!(early.warnings.contains(&RecordWarning::CompletedLongAfter));
    attempt(day(1900, 1, 1), &|_| {}).expect("the floor itself");
}

/// An edit names the version it read; a stale one is refused, a current one
/// bumps it and stamps the editor.
#[test]
fn edits_are_versioned() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let asset = new_asset(&pool, ws, "Camera 20");
    let repair = repair_type(&pool, ws);
    let created = in_ws(&pool, ws, |c| {
        records::create(c, record(asset, repair.id, day(2024, 2, 1), None))
    })
    .expect("create")
    .record;

    let edit = |version: i32| {
        in_ws(&pool, ws, |c| {
            records::update(
                c,
                created.id,
                version,
                RecordChanges {
                    vendor: Some(Some("Lens Lab".into())),
                    occurred: Some(WorkDate::Day(day(2024, 1, 30))),
                    ..Default::default()
                },
                Some(seeded.a.member_uuid),
            )
        })
    };
    let edited = edit(1).expect("edit at the version read").record;
    assert_eq!(edited.version, 2);
    assert_eq!(edited.vendor.as_deref(), Some("Lens Lab"));
    assert_eq!(edited.occurred_on, day(2024, 1, 30));
    assert_eq!(edited.updated_by, Some(seeded.a.member_uuid));
    assert_eq!(
        (edited.recorded_at, edited.recorded_by),
        (created.recorded_at, created.recorded_by),
        "the entry clock and author stay"
    );
    assert!(matches!(
        refused(edit(1)),
        RecordError::VersionConflict { current: 2 }
    ));
}

/// The app role may edit what an edit changes, never when or by whom a
/// record was entered or where it came from.
#[test]
fn the_app_role_cannot_rewrite_the_entry() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let asset = new_asset(&pool, ws, "Camera 21");
    let repair = repair_type(&pool, ws);
    in_ws(&pool, ws, |c| {
        records::create(c, record(asset, repair.id, day(2024, 2, 1), None))
    })
    .expect("create");

    in_ws(&pool, ws, |c| {
        diesel::sql_query("UPDATE asset_records SET summary = 'Edited'").execute(c)
    })
    .expect("an editable column");
    for sql in [
        "UPDATE asset_records SET recorded_at = now() - interval '1 year'",
        "UPDATE asset_records SET recorded_by = NULL",
        "UPDATE asset_records SET source = 'import'",
        "UPDATE asset_records SET asset_id = asset_id",
    ] {
        let err = run_in_workspace(&pool, REF, ws, |c| diesel::sql_query(sql).execute(c))
            .expect_err("refused");
        assert!(
            err.to_string().contains("permission denied"),
            "{sql}: {err}"
        );
    }
}

/// A record and its type stay in their workspace.
#[test]
fn records_stay_in_their_workspace() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    let asset = new_asset(&pool, a, "Camera 22");
    let repair = repair_type(&pool, a);
    let created = in_ws(&pool, a, |c| {
        records::create(c, record(asset, repair.id, day(2024, 2, 1), None))
    })
    .expect("create")
    .record;

    assert!(in_ws(&pool, b, |c| records::get(c, created.id))
        .expect("read")
        .is_none());
    assert!(in_ws(&pool, b, |c| records::list_for_asset(c, asset))
        .expect("list")
        .is_empty());
    assert!(in_ws(&pool, b, |c| asset_record_types::get(c, repair.id))
        .expect("read")
        .is_none());
    let other = new_asset(&pool, b, "Camera 23");
    assert!(matches!(
        refused(in_ws(&pool, b, |c| {
            records::create(c, record(other, repair.id, day(2024, 2, 1), None))
        })),
        RecordError::RecordTypeNotFound
    ));
}

/// Files point at their record, and stay on the asset when it goes.
/// Deleting the asset takes its records; hard-deleting the workspace takes
/// records and types.
#[test]
fn files_stay_and_deletes_cascade() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.b.workspace_id;
    let asset = new_asset(&pool, ws, "Camera 24");
    let repair = repair_type(&pool, ws);
    let created = in_ws(&pool, ws, |c| {
        records::create(c, record(asset, repair.id, day(2024, 2, 1), None))
    })
    .expect("create")
    .record;
    let media: Vec<i32> = in_ws(&pool, ws, |c| {
        let mut ids = Vec::new();
        for name in ["invoice.pdf", "report.pdf"] {
            ids.push(
                diesel::sql_query(
                    "INSERT INTO asset_media (asset_id, url, name, kind) \
                     VALUES ($1, '/files/x', $2, 'document') RETURNING id",
                )
                .bind::<Integer, _>(asset)
                .bind::<Text, _>(name)
                .get_result::<IdRow>(c)?
                .id,
            );
        }
        QueryResult::Ok(ids)
    })
    .expect("two documents");

    let linked = in_ws(&pool, ws, |c| records::set_files(c, created.id, &media)).expect("link");
    assert_eq!(linked.len(), 2);
    let linked = in_ws(&pool, ws, |c| {
        records::set_files(c, created.id, &media[..1])
    })
    .expect("relink one");
    assert_eq!(
        linked.iter().map(|m| m.id).collect::<Vec<_>>(),
        media[..1].to_vec()
    );

    in_ws(&pool, ws, |c| records::delete(c, created.id))
        .expect("delete")
        .expect("it existed");
    let left: Vec<Option<i32>> = in_ws(&pool, ws, |c| {
        media_t::table
            .filter(media_t::asset_id.eq(asset))
            .select(media_t::record_id)
            .load(c)
    })
    .expect("files");
    assert_eq!(left, vec![None, None], "the files stay, unlinked");
    assert!(
        in_ws(&pool, ws, |c| asset_media::list_for_record(c, created.id))
            .expect("list")
            .is_empty()
    );

    let second = in_ws(&pool, ws, |c| {
        records::create(c, record(asset, repair.id, day(2024, 3, 1), None))
    })
    .expect("create")
    .record;
    in_ws(&pool, ws, |c| asset_repo::delete_device(c, asset, None)).expect("delete the asset");
    let mut conn = pool.get().expect("conn");
    let left: i64 = with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        asset_records::table
            .filter(asset_records::id.eq(second.id))
            .count()
            .get_result(c)
    })
    .expect("count");
    assert_eq!(left, 0, "deleting the asset takes its records");

    let kept = new_asset(&pool, ws, "Camera 25");
    in_ws(&pool, ws, |c| {
        records::create(c, record(kept, repair.id, day(2024, 3, 1), None))
    })
    .expect("create");
    let deleted = with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        workspaces::archive_workspace(c, ws)?;
        workspaces::hard_delete_workspace(c, ws, Utc::now() + Duration::hours(1))
    })
    .expect("hard-delete a workspace whose type has records");
    assert_eq!(deleted, 1);
}

/// Text longer than its column is refused by field, and a source reference
/// another record holds answers with that record, even when a concurrent
/// write takes it between the check and the insert.
#[test]
fn long_text_and_a_taken_source_ref_are_refused_by_name() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(3);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let asset = new_asset(&pool, ws, "Camera 26");
    let repair = repair_type(&pool, ws);
    let with_ref = move |source_ref: &str| {
        let mut input = record(asset, repair.id, day(2024, 2, 1), None);
        input.source_ref = Some(source_ref.to_string());
        input
    };

    let mut long = record(asset, repair.id, day(2024, 2, 1), None);
    long.vendor = Some("v".repeat(256));
    assert!(matches!(
        refused(in_ws(&pool, ws, |c| records::create(c, long))),
        RecordError::TooLong {
            field: "vendor",
            max: 255
        }
    ));

    let first = in_ws(&pool, ws, |c| records::create(c, with_ref("legacy-1")))
        .expect("create")
        .record;
    assert!(matches!(
        refused(in_ws(&pool, ws, |c| records::create(c, with_ref("legacy-1")))),
        RecordError::SourceRefExists { id } if id == first.id
    ));

    // The holder writes and waits before committing; the second write passes
    // the check, then meets the unique index once the holder commits.
    let (written, wait) = std::sync::mpsc::channel();
    let holder_pool = pool.clone();
    let holder = std::thread::spawn(move || {
        in_ws(&holder_pool, ws, |c| {
            let w = records::create(c, with_ref("legacy-2")).expect("the first write");
            written.send(()).expect("signal");
            std::thread::sleep(std::time::Duration::from_millis(800));
            w.record.id
        })
    });
    wait.recv().expect("the holder wrote");
    let second = in_ws(&pool, ws, |c| records::create(c, with_ref("legacy-2")));
    let winner = holder.join().expect("the holder");
    assert!(
        matches!(refused(second), RecordError::SourceRefExists { id } if id == winner),
        "the race answers with the record that won"
    );
}

/// A new record on an archived type is refused for the type before its
/// source reference is looked at, as it was before the importer shared the
/// checks.
#[test]
fn an_archived_type_is_refused_before_a_taken_source_ref() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let asset = new_asset(&pool, ws, "Camera");
    let repair = repair_type(&pool, ws);
    let mut first = record(asset, repair.id, day(2024, 2, 1), None);
    first.source_ref = Some("legacy-9".to_string());
    in_ws(&pool, ws, |c| records::create(c, first.clone())).expect("create");
    in_ws(&pool, ws, |c| asset_record_types::archive(c, repair.id)).expect("archive");
    assert!(matches!(
        refused(in_ws(&pool, ws, |c| records::create(c, first))),
        RecordError::RecordTypeArchived
    ));
}

/// Deleting the transition that opened a record clears the link, through
/// the index on it, and leaves the record.
#[test]
fn deleting_the_opening_transition_clears_the_link() {
    #[derive(QueryableByName)]
    struct Found {
        #[diesel(sql_type = diesel::sql_types::Bool)]
        found: bool,
    }
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let asset = new_asset(&pool, ws, "Camera 27");
    let repair = repair_type(&pool, ws);
    let event = in_ws(&pool, ws, |c| {
        diesel::sql_query(
            "INSERT INTO asset_lifecycle_events (asset_id, from_status, to_status) \
             VALUES ($1, 'in_service', 'in_repair') RETURNING id",
        )
        .bind::<Integer, _>(asset)
        .get_result::<IdRow>(c)
    })
    .expect("a transition")
    .id;
    let mut input = record(asset, repair.id, Utc::now().date_naive(), None);
    input.completed = None;
    input.outcome = None;
    input.source = RecordSource::Transition;
    input.lifecycle_event_id = Some(event);
    let opened = in_ws(&pool, ws, |c| records::create(c, input))
        .expect("an open repair")
        .record;
    assert_eq!(opened.lifecycle_event_id, Some(event));

    let mut conn = pool.get().expect("conn");
    let indexed = with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        diesel::sql_query(
            "SELECT EXISTS (SELECT 1 FROM pg_indexes WHERE tablename = 'asset_records' \
             AND indexdef LIKE '%(workspace_id, lifecycle_event_id)%') AS found",
        )
        .get_result::<Found>(c)
    })
    .expect("indexes")
    .found;
    assert!(indexed, "the link is indexed for the delete");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        diesel::sql_query("DELETE FROM asset_lifecycle_events WHERE id = $1")
            .bind::<Integer, _>(event)
            .execute(c)
    })
    .expect("delete the transition");
    let kept = in_ws(&pool, ws, |c| records::get(c, opened.id))
        .expect("read")
        .expect("the record stays");
    assert_eq!(kept.lifecycle_event_id, None);
}
