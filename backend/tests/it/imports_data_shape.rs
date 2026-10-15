//! What a commit stores per written row, the Imported lifecycle event, the
//! commit guards (job and type locks, replan and compare), constants, and
//! the saved-answers table.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::models::NewImportValueAnswer;
use backend::repository::imports as repo;
use backend::services::imports::{self, csv_parser, FieldChange, ImportType, RecordId, RowOutcome};
use backend::sync::actor::ActorContext;
use backend::sync::session::{pin_workspace, run_in_workspace, with_actor_bypass_context};

use crate::common::{import_ctx, TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{
    call_import_route, import_workspace, import_workspaces, json, mapping_request,
    template_mapping, upload_and_check, upload_request,
};

const HEADER: &str =
    "name,kind,asset_tag,serial_number,manufacturer,model,location,notes,quantity,unit,low_stock_threshold";
const REF: &str = "test:imports_data_shape";

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = Integer)]
    id: i32,
}

fn asset_id_by_tag(conn: &mut diesel::PgConnection, tag: &str) -> i32 {
    diesel::sql_query("SELECT id FROM assets WHERE asset_tag = $1")
        .bind::<Text, _>(tag)
        .get_result::<IdRow>(conn)
        .expect("read asset id")
        .id
}

fn seed_asset_in(
    conn: &mut diesel::PgConnection,
    workspace_id: i32,
    name: &str,
    tag: &str,
    location: &str,
) -> i32 {
    diesel::sql_query(
        "INSERT INTO assets (workspace_id, name, kind, asset_tag, location, notes)
         VALUES ($1, $2, 'generic', $3, $4, 'kept')",
    )
    .bind::<Integer, _>(workspace_id)
    .bind::<Text, _>(name)
    .bind::<Text, _>(tag)
    .bind::<Text, _>(location)
    .execute(conn)
    .expect("seed asset");
    asset_id_by_tag(conn, tag)
}

/// `assets.updated_at` in the form row results keep it.
fn updated_at_text(conn: &mut diesel::PgConnection, id: i32) -> String {
    #[derive(QueryableByName)]
    struct At {
        #[diesel(sql_type = Text)]
        at: String,
    }
    diesel::sql_query(
        "SELECT to_char(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US') AS at FROM assets WHERE id = $1",
    )
    .bind::<Integer, _>(id)
    .get_result::<At>(conn)
    .expect("read updated_at")
    .at
}

#[derive(QueryableByName, Debug)]
struct EventRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
    from_status: Option<String>,
    #[diesel(sql_type = Text)]
    to_status: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
    reason: Option<String>,
    #[diesel(sql_type = diesel::sql_types::Jsonb)]
    metadata: serde_json::Value,
    #[diesel(sql_type = diesel::sql_types::Nullable<SqlUuid>)]
    actor_uuid: Option<Uuid>,
}

fn events_for(conn: &mut diesel::PgConnection, asset_id: i32) -> Vec<EventRow> {
    diesel::sql_query(
        "SELECT id, from_status, to_status, reason, metadata, actor_uuid
         FROM asset_lifecycle_events WHERE asset_id = $1 ORDER BY id",
    )
    .bind::<Integer, _>(asset_id)
    .load(conn)
    .expect("read lifecycle events")
}

/// A committed update keeps only the fields it changed, before and after
/// (a blank cell is never among them); a committed create keeps every
/// field it wrote with no before, and an Imported lifecycle event naming
/// the job. Both keep the commit time and the asset's `updated_at` as read
/// back.
#[test]
fn committed_rows_record_what_they_wrote() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let existing = seed_asset_in(&mut conn, 1, "Camera 1", "SHAPE-1", "Studio A");
    let actor = crate::imports_users::new_member(&mut conn, 1, "admin");

    // Row 2 renames nothing, moves the camera and leaves notes blank; row 3
    // is new.
    let parsed = csv_parser::parse_bytes(
        format!(
            "{HEADER}\n\
             Camera 1,generic,SHAPE-1,,,,Studio B,,,,\n\
             Tripod,generic,SHAPE-2,SN-2,,,Store,,2,pcs,\n"
        )
        .as_bytes(),
    )
    .expect("parse");
    let mut ctx = import_ctx();
    ctx.actor = Some(actor);

    let committed = imports::apply(&mut conn, ImportType::Assets, &parsed, &ctx).expect("commit");
    let [update, create] = committed.rows.as_slice() else {
        panic!("two rows: {:?}", committed.rows);
    };

    assert_eq!(update.outcome, RowOutcome::Update);
    assert_eq!(
        update.fields_written.iter().collect::<Vec<_>>(),
        vec![(
            &"location".to_string(),
            &FieldChange {
                before: Some("Studio A".into()),
                after: Some("Studio B".into())
            }
        )]
    );
    assert_eq!(update.transition_id, None);
    assert!(update.written_at.is_some());
    assert_eq!(
        update.observed_updated_at.as_deref(),
        Some(updated_at_text(&mut conn, existing).as_str())
    );
    assert!(
        events_for(&mut conn, existing).is_empty(),
        "an update logs no event"
    );

    let created = asset_id_by_tag(&mut conn, "SHAPE-2");
    assert_eq!(create.outcome, RowOutcome::Create);
    assert_eq!(create.record_id, Some(RecordId::Int(created)));
    let written: Vec<(&str, Option<&str>, Option<&str>)> = create
        .fields_written
        .iter()
        .map(|(k, v)| (k.as_str(), v.before.as_deref(), v.after.as_deref()))
        .collect();
    assert_eq!(
        written,
        vec![
            ("asset_tag", None, Some("SHAPE-2")),
            ("kind", None, Some("generic")),
            ("location", None, Some("Store")),
            ("name", None, Some("Tripod")),
            ("quantity", None, Some("2.000")),
            ("serial_number", None, Some("SN-2")),
            ("unit", None, Some("pcs")),
        ]
    );
    assert_eq!(create.written_at, update.written_at, "one commit time");
    assert_eq!(
        create.observed_updated_at.as_deref(),
        Some(updated_at_text(&mut conn, created).as_str())
    );

    let events = events_for(&mut conn, created);
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(create.transition_id, Some(event.id));
    assert_eq!(event.from_status, None);
    assert_eq!(event.to_status, "in_service");
    assert_eq!(event.reason.as_deref(), Some("Imported"));
    assert_eq!(
        event.metadata,
        serde_json::json!({ "import_job_id": ctx.job_id })
    );
    assert_eq!(event.actor_uuid, Some(actor));

    // The event reached sync like any other lifecycle event.
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    let synced = diesel::sql_query(
        "SELECT COUNT(*) AS count FROM sync_actions
         WHERE aggregate::text = 'asset_lifecycle_event' AND aggregate_id = $1",
    )
    .bind::<Text, _>(event.id.to_string())
    .get_result::<Count>(&mut conn)
    .expect("count sync actions")
    .count;
    assert_eq!(synced, 1);
}

/// The commit route.
fn commit_request(job_id: &str) -> TestRequest {
    TestRequest::post().uri(&format!("/api/admin/import/{job_id}/commit"))
}

#[derive(QueryableByName)]
struct JobState {
    #[diesel(sql_type = Text)]
    status: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
    record_ids: Option<serde_json::Value>,
}

fn job_state(pool: &TestPool, job_id: &str) -> JobState {
    diesel::sql_query("SELECT status, record_ids FROM import_jobs WHERE id = $1::uuid")
        .bind::<Text, _>(job_id)
        .get_result(&mut pool.get().expect("conn"))
        .expect("read job")
}

fn tag_count(pool: &TestPool, tag: &str) -> i64 {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    diesel::sql_query("SELECT COUNT(*) AS count FROM assets WHERE asset_tag = $1")
        .bind::<Text, _>(tag)
        .get_result::<Count>(&mut pool.get().expect("conn"))
        .expect("count")
        .count
}

/// An asset changed between the check and the commit so that a row would
/// come to something else: the commit writes nothing, answers 409 with the
/// fresh plan, and keeps it as the plan to review. Importing again commits
/// it and records the ids it created and updated.
#[actix_web::test]
async fn a_plan_changed_since_the_check_is_refused_with_the_fresh_plan() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let existing = seed_asset_in(
        &mut pool.get().expect("conn"),
        ws.workspace_id,
        "Camera 1",
        "PLAN-1",
        "Studio A",
    );

    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nCamera 1,generic,PLAN-1,,,,Studio B,,,,\nNew camera,generic,PLAN-2,,,,,,,,\n"),
    )
    .await;
    let job_id = job["id"].as_str().expect("job id").to_string();
    assert_eq!(
        (
            &job["summary"]["would_create"],
            &job["summary"]["would_update"]
        ),
        (&serde_json::json!(1), &serde_json::json!(1))
    );

    // Someone adds an asset with the tag the file was going to create.
    let added = seed_asset_in(
        &mut pool.get().expect("conn"),
        ws.workspace_id,
        "Added meanwhile",
        "PLAN-2",
        "Store",
    );

    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, commit_request(&job_id)).await;
    let body = json(&body);
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_plan_changed", "{body}");
    assert_eq!(body["job"]["status"], "dry_run_done", "{body}");
    assert_eq!(body["job"]["summary"]["would_create"], 0, "{body}");
    assert_eq!(body["job"]["summary"]["would_update"], 2, "{body}");

    // Nothing was written: the existing camera kept its location.
    #[derive(QueryableByName)]
    struct Location {
        #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
        location: Option<String>,
    }
    let location: Location = diesel::sql_query("SELECT location FROM assets WHERE id = $1")
        .bind::<Integer, _>(existing)
        .get_result(&mut pool.get().expect("conn"))
        .expect("read location");
    assert_eq!(location.location.as_deref(), Some("Studio A"));
    assert_eq!(job_state(&pool, &job_id).status, "dry_run_done");

    // The job now holds the fresh plan, so importing again commits it.
    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, commit_request(&job_id)).await;
    let done = json(&body);
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["status"], "done");
    assert_eq!(
        tag_count(&pool, "PLAN-2"),
        1,
        "the row updated the added asset"
    );
    let state = job_state(&pool, &job_id);
    assert_eq!(
        state.record_ids,
        Some(serde_json::json!({ "created": [], "updated": [existing, added] }))
    );

    // The audit log says the id lists changed without holding them.
    #[derive(QueryableByName)]
    struct Audit {
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
        after_jsonb: Option<serde_json::Value>,
    }
    let last: Audit = diesel::sql_query(
        "SELECT after_jsonb FROM audit_log
         WHERE table_name = 'import_jobs' AND pk_text = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind::<Text, _>(&job_id)
    .get_result(&mut pool.get().expect("conn"))
    .expect("read audit");
    let after = last.after_jsonb.expect("after");
    assert_eq!(after["status"], "done", "{after}");
    assert_eq!(after["record_ids_changed"], true, "{after}");
    assert!(after.get("record_ids").is_none(), "{after}");
    assert!(after.get("answers").is_none(), "{after}");
}

/// A commit with nothing changed since the check stores what each row wrote
/// and the ids it created.
#[actix_web::test]
async fn an_unchanged_plan_commits_and_keeps_the_written_rows() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nFresh camera,generic,KEEP-9,,,,,,,,\n"),
    )
    .await;
    let job_id = job["id"].as_str().expect("job id").to_string();
    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, commit_request(&job_id)).await;
    assert_eq!(status, StatusCode::OK, "{}", json(&body));

    let created = asset_id_by_tag(&mut pool.get().expect("conn"), "KEEP-9");
    assert_eq!(
        job_state(&pool, &job_id).record_ids,
        Some(serde_json::json!({ "created": [created], "updated": [] }))
    );
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{job_id}/rows")),
    )
    .await;
    let page = json(&body);
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["phase"], "committed");
    let row = &page["rows"][0];
    assert_eq!(row["outcome"], "create", "{row}");
    assert_eq!(
        row["fields_written"]["name"]["after"], "Fresh camera",
        "{row}"
    );
    assert!(row["transition_id"].is_i64(), "{row}");
    assert!(row["written_at"].is_string(), "{row}");
    assert!(row["observed_updated_at"].is_string(), "{row}");
}

/// Begin a transaction on its own connection and take a lock in it; the
/// lock holds until the connection rolls back.
fn hold_lock(
    pool: &TestPool,
    workspace_id: i32,
    take: impl FnOnce(&mut crate::common::TestPooledConn) -> QueryResult<bool>,
) -> crate::common::TestPooledConn {
    let mut conn = pool.get().expect("conn");
    diesel::sql_query("BEGIN")
        .execute(&mut conn)
        .expect("begin");
    pin_workspace(&mut conn, workspace_id).expect("pin");
    assert!(take(&mut conn).expect("lock"), "the lock was free");
    conn
}

fn release(mut conn: crate::common::TestPooledConn) {
    diesel::sql_query("ROLLBACK")
        .execute(&mut conn)
        .expect("rollback");
}

/// A second commit of a job whose commit is running is refused, not queued;
/// the job reads as committing meanwhile, and its mapping can't change. A
/// commit while another import of the same type commits into the workspace
/// is refused too. Once the locks are released the commit goes through.
#[actix_web::test]
async fn a_running_commit_refuses_a_second_one() {
    let db = TestDb::new();
    let pool = db.pool_with_size(6);
    let ws = import_workspace(&pool);
    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nLocked,generic,LOCK-1,,,,,,,,\n"),
    )
    .await;
    let job_id = job["id"].as_str().expect("job id").to_string();
    let id = Uuid::parse_str(&job_id).expect("uuid");

    // A mapping change holds the job's lock alone: the job doesn't read as
    // committing, and a commit is refused until the change lands.
    let held = hold_lock(&pool, ws.workspace_id, |c| repo::try_lock_job(c, id));
    let (_, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{job_id}")),
    )
    .await;
    assert_eq!(json(&body)["status"], "dry_run_done");
    let (status, _) = call_import_route(&pool, &ws, ws.admin_uuid, commit_request(&job_id)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    release(held);

    // What a running commit holds: its mark and the job's lock.
    let held = hold_lock(&pool, ws.workspace_id, |c| {
        Ok(repo::try_mark_commit(c, id)? && repo::try_lock_job(c, id)?)
    });
    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, commit_request(&job_id)).await;
    let body = json(&body);
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_commit_running", "{body}");

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{job_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["status"], "committing");

    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(&job_id, template_mapping()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    release(held);

    let held = hold_lock(&pool, ws.workspace_id, |c| {
        repo::try_lock_type(c, ws.workspace_id, "assets")
    });
    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, commit_request(&job_id)).await;
    let body = json(&body);
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_type_busy", "{body}");
    assert_eq!(tag_count(&pool, "LOCK-1"), 0);
    release(held);

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{job_id}")),
    )
    .await;
    assert_eq!(json(&body)["status"], "dry_run_done", "{status}");
    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, commit_request(&job_id)).await;
    assert_eq!(status, StatusCode::OK, "{}", json(&body));
    assert_eq!(tag_count(&pool, "LOCK-1"), 1);
}

/// Upload a template file, then map it with `options`; the response.
async fn upload_and_map(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    file: &str,
    options: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        upload_request("assets.csv", file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let id = job["id"].as_str().expect("job id").to_string();
    // The template mapping minus location, which a constant sets.
    let mut body = template_mapping();
    body["mapping"]
        .as_object_mut()
        .expect("mapping")
        .remove("location");
    body["options"] = options;
    let (status, body) =
        call_import_route(pool, ws, ws.admin_uuid, mapping_request(&id, body)).await;
    (status, json(&body))
}

fn location_of(pool: &TestPool, tag: &str) -> Option<String> {
    #[derive(QueryableByName)]
    struct Location {
        #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
        location: Option<String>,
    }
    diesel::sql_query("SELECT location FROM assets WHERE asset_tag = $1")
        .bind::<Text, _>(tag)
        .get_result::<Location>(&mut pool.get().expect("conn"))
        .expect("read location")
        .location
}

/// A constant fills its field on every new row; on a row matching an
/// existing asset it writes only when it applies to all rows.
#[actix_web::test]
async fn constants_set_new_rows_and_existing_ones_only_when_asked() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    for tag in ["CONST-1", "CONST-3"] {
        seed_asset_in(
            &mut pool.get().expect("conn"),
            ws.workspace_id,
            tag,
            tag,
            "Studio A",
        );
    }

    let (status, job) = upload_and_map(
        &pool,
        &ws,
        &format!("{HEADER}\nCONST-1,generic,CONST-1,,,,,,,,\nNew one,generic,CONST-2,,,,,,,,\n"),
        serde_json::json!({ "constants": [{ "field": "location", "value": " Studio B " }] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["options"]["constants"][0]["apply_to"], "new", "{job}");
    let job_id = job["id"].as_str().expect("job id").to_string();
    let (status, _) = call_import_route(&pool, &ws, ws.admin_uuid, commit_request(&job_id)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(location_of(&pool, "CONST-1").as_deref(), Some("Studio A"));
    assert_eq!(location_of(&pool, "CONST-2").as_deref(), Some("Studio B"));

    let (status, job) = upload_and_map(
        &pool,
        &ws,
        &format!("{HEADER}\nCONST-3,generic,CONST-3,,,,,,,,\n"),
        serde_json::json!({ "constants": [{ "field": "location", "value": "Studio C", "apply_to": "all" }] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let job_id = job["id"].as_str().expect("job id").to_string();
    let (status, _) = call_import_route(&pool, &ws, ws.admin_uuid, commit_request(&job_id)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(location_of(&pool, "CONST-3").as_deref(), Some("Studio C"));
}

/// The Map step refuses a constant that can't apply: an unknown field, one
/// a column feeds too, a match key, a blank value, an unknown apply-to.
#[actix_web::test]
async fn constants_are_validated_at_the_map_step() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let file = format!("{HEADER}\nA,generic,VAL-1,,,,,,,,\n");
    let cases = [
        (
            serde_json::json!([{ "field": "nope", "value": "x" }]),
            "unknown field",
        ),
        (
            serde_json::json!([{ "field": "name", "value": "x" }]),
            "comes from a column",
        ),
        (
            serde_json::json!([{ "field": "location", "value": "  " }]),
            "is blank",
        ),
        (
            serde_json::json!([{ "field": "location", "value": "a" }, { "field": "location", "value": "b" }]),
            "twice",
        ),
    ];
    for (constants, expected) in cases {
        let (status, body) = upload_and_map(
            &pool,
            &ws,
            &file,
            serde_json::json!({ "constants": constants }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{constants}: {body}");
        assert!(
            body["error"].as_str().unwrap_or("").contains(expected),
            "{constants}: {body}"
        );
    }
    let (status, body) = upload_and_map(
        &pool,
        &ws,
        &file,
        serde_json::json!({ "constants": [{ "field": "location", "value": "x", "apply_to": "some" }] }),
    )
    .await;
    assert_ne!(status, StatusCode::OK, "{body}");
}

fn answer(field: &str, value: &str, target: &str, by: Option<Uuid>) -> NewImportValueAnswer {
    NewImportValueAnswer {
        job_type: "assets".to_string(),
        field: field.to_string(),
        context: String::new(),
        normalized_value: value.to_string(),
        answer: serde_json::json!({ "action": "match", "target": target }),
        answered_by: by,
        job_id: None,
    }
}

/// Saved answers belong to their workspace: another workspace's session
/// sees none of them. Saving the same field, context and value again
/// replaces the answer. Purging the person who answered keeps the answer;
/// hard-deleting the workspace removes it.
#[test]
fn saved_answers_are_per_workspace_and_follow_their_owners() {
    let db = TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = import_workspaces(&pool);
    let (a, b) = (&seeded.a, &seeded.b);
    let runtime = db.runtime_pool(2);

    run_in_workspace(&runtime, REF, a.workspace_id, |c| {
        repo::upsert_value_answers(
            c,
            &[
                answer("kind", "cam", "camera", Some(a.member_uuid)),
                answer("kind", "spare", "generic", Some(a.member_uuid)),
            ],
        )
    })
    .expect("save in a");
    run_in_workspace(&runtime, REF, b.workspace_id, |c| {
        repo::upsert_value_answers(c, &[answer("kind", "cam", "other", None)])
    })
    .expect("save in b");
    let replaced = run_in_workspace(&runtime, REF, a.workspace_id, |c| {
        repo::upsert_value_answers(c, &[answer("kind", "cam", "generic", Some(a.admin_uuid))])
    })
    .expect("replace in a");
    assert_eq!(replaced, 1);

    let in_a = run_in_workspace(&runtime, REF, a.workspace_id, |c| {
        repo::list_value_answers(c, "assets")
    })
    .expect("list a");
    let got: Vec<(&str, &serde_json::Value, Option<Uuid>)> = in_a
        .iter()
        .map(|r| {
            (
                r.normalized_value.as_str(),
                &r.answer["target"],
                r.answered_by,
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("cam", &serde_json::json!("generic"), Some(a.admin_uuid)),
            ("spare", &serde_json::json!("generic"), Some(a.member_uuid)),
        ]
    );
    let in_b = run_in_workspace(&runtime, REF, b.workspace_id, |c| {
        repo::list_value_answers(c, "assets")
    })
    .expect("list b");
    assert_eq!(in_b.len(), 1);
    assert_eq!(in_b[0].answer["target"], "other");
    // A workspace can't delete another's answers.
    let deleted = run_in_workspace(&runtime, REF, b.workspace_id, |c| {
        repo::delete_value_answer(c, in_a[1].id)
    })
    .expect("delete across");
    assert_eq!(deleted, 0);

    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        pin_workspace(c, a.workspace_id)?;
        backend::repository::users::purge_user(&a.member_uuid, c, None)
    })
    .expect("purge the member");
    let in_a = run_in_workspace(&runtime, REF, a.workspace_id, |c| {
        repo::list_value_answers(c, "assets")
    })
    .expect("list a after purge");
    assert_eq!(in_a.len(), 2, "the answers outlive who gave them");
    assert_eq!(in_a[1].answered_by, None);

    let deleted = with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        backend::repository::workspaces::archive_workspace(c, b.workspace_id)?;
        backend::repository::workspaces::hard_delete_workspace(
            c,
            b.workspace_id,
            chrono::Utc::now() + chrono::Duration::hours(1),
        )
    })
    .expect("hard-delete b");
    assert_eq!(deleted, 1);
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    let left = diesel::sql_query(
        "SELECT COUNT(*) AS count FROM import_value_answers WHERE workspace_id = $1",
    )
    .bind::<Integer, _>(b.workspace_id)
    .get_result::<Count>(&mut conn)
    .expect("count")
    .count;
    assert_eq!(left, 0);
}
