//! Restore previous values (`POST /admin/import/{id}/restore`): a checked,
//! update-only draft that puts back what an import's updates replaced,
//! leaving a value someone changed since; the commit through the ordinary
//! audited, emitting path; the open draft found again; the links between
//! the two detail pages; and the refusals.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Nullable, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::sync::session::run_in_workspace;

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{call_import_route, import_workspace, import_workspaces, json};
use crate::imports_attributes::{checked, commit, id_of, rows};

const REF: &str = "test:imports_restore";

const MAPPING: &[(&str, &str)] = &[
    ("Name", "name"),
    ("Tag", "asset_tag"),
    ("Location", "location"),
    ("Notes", "notes"),
    ("Status", "status"),
];

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count(pool: &TestPool, sql: &str) -> i64 {
    diesel::sql_query(sql)
        .get_result::<Count>(&mut pool.get().expect("conn"))
        .expect(sql)
        .n
}

fn exec(pool: &TestPool, sql: &str) {
    diesel::sql_query(sql)
        .execute(&mut pool.get().expect("conn"))
        .expect(sql);
}

#[derive(QueryableByName, Debug, PartialEq)]
struct Asset {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Nullable<Text>)]
    location: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    notes: Option<String>,
    #[diesel(sql_type = Text)]
    status: String,
}

fn asset(pool: &TestPool, tag: &str) -> Asset {
    diesel::sql_query("SELECT id, location, notes, status FROM assets WHERE asset_tag = $1")
        .bind::<Text, _>(tag)
        .get_result(&mut pool.get().expect("conn"))
        .expect("read asset")
}

fn seed(pool: &TestPool, ws: &WorkspaceSeed, tag: &str, location: &str, notes: &str) {
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, location, notes, status, workspace_id)
         VALUES ($1, 'generic', $1, $2, $3, 'in_service', $4)",
    )
    .bind::<Text, _>(tag)
    .bind::<Text, _>(location)
    .bind::<Text, _>(notes)
    .bind::<Integer, _>(ws.workspace_id)
    .execute(&mut pool.get().expect("conn"))
    .expect("seed asset");
}

/// Move V-1 and V-2 to Field kit 2 with new notes and V-1 to In repair, and
/// create V-3: the committed job's id.
async fn update_import(pool: &TestPool, ws: &WorkspaceSeed) -> String {
    seed(pool, ws, "V-1", "Studio A", "Clean");
    seed(pool, ws, "V-2", "Studio B", "Spare");
    let file = "Name,Tag,Location,Notes,Status\n\
                V-1,V-1,Field kit 2,Dented,in_repair\n\
                V-2,V-2,Field kit 2,Loaned,\n\
                V-3,V-3,Field kit 2,,\n";
    let job = checked(
        pool,
        ws,
        file,
        MAPPING,
        serde_json::json!({ "default_kind": "generic" }),
    )
    .await;
    assert_eq!(job["summary"]["would_update"], 2, "{job}");
    let id = id_of(&job);
    commit(pool, ws, &id).await;
    id
}

async fn call(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    user: Uuid,
    req: TestRequest,
) -> (StatusCode, serde_json::Value) {
    let (status, body) = call_import_route(pool, ws, user, req).await;
    (status, json(&body))
}

async fn restore(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> (StatusCode, serde_json::Value) {
    call(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/restore")),
    )
    .await
}

async fn get_job(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> serde_json::Value {
    let (status, job) = call(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    job
}

fn notice(job: &serde_json::Value, code: &str) -> Option<u64> {
    job["summary"]["notices"]
        .as_array()?
        .iter()
        .find(|n| n["code"] == code)
        .and_then(|n| n["count"].as_u64())
}

/// The restore is an update-only draft on the source's key, checked and on
/// Review with the reverse changes; committing it puts the values back
/// through the asset updates' sync actions, and each detail page links to
/// the other.
#[actix_web::test]
async fn a_restore_opens_on_review_and_puts_the_values_back() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let source = update_import(&pool, &ws).await;
    let v1 = asset(&pool, "V-1");
    assert_eq!(v1.location.as_deref(), Some("Field kit 2"));
    assert_eq!(v1.status, "in_repair");

    let (status, draft) = restore(&pool, &ws, &source).await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    assert_eq!(draft["status"], "dry_run_done", "{draft}");
    assert_eq!(draft["state"], "draft", "{draft}");
    assert_eq!(draft["restore_of"], source.as_str(), "{draft}");
    assert_eq!(draft["options"]["mode"], "update_only", "{draft}");
    assert_eq!(draft["options"]["match_by"], "asset_tag", "{draft}");
    assert_eq!(
        draft["options"]["constants"],
        serde_json::json!([]),
        "{draft}"
    );
    assert_eq!(
        draft["mapping"],
        serde_json::json!({ "asset_tag": 0, "status": 1, "location": 2, "notes": 3 }),
        "{draft}"
    );
    assert_eq!(draft["summary"]["would_update"], 2, "{draft}");
    assert_eq!(draft["summary"]["would_create"], 0, "{draft}");
    assert_eq!(draft["summary"]["rows_with_errors"], 0, "{draft}");
    assert_eq!(notice(&draft, "restore_changed_since"), None, "{draft}");
    let id = id_of(&draft);

    let planned = rows(&pool, &ws, &id).await;
    let v1_row = planned.iter().find(|r| r["key"] == "V-1").expect("V-1 row");
    assert_eq!(
        v1_row["fields_written"]["location"],
        serde_json::json!({ "before": "Field kit 2", "after": "Studio A" }),
        "{v1_row}"
    );
    assert_eq!(
        v1_row["fields_written"]["status"],
        serde_json::json!({ "before": "in_repair", "after": "in_service" }),
        "{v1_row}"
    );

    let updates_before = count(
        &pool,
        &format!(
            "SELECT COUNT(*) AS n FROM sync_actions
             WHERE event_type = 'asset.updated' AND aggregate_id = '{}'",
            v1.id
        ),
    );
    let done = commit(&pool, &ws, &id).await;
    assert_eq!(done["status"], "done", "{done}");
    assert_eq!(done["restores"]["id"], source.as_str(), "{done}");

    let v1 = asset(&pool, "V-1");
    assert_eq!(v1.location.as_deref(), Some("Studio A"));
    assert_eq!(v1.notes.as_deref(), Some("Clean"));
    assert_eq!(v1.status, "in_service");
    let v2 = asset(&pool, "V-2");
    assert_eq!(v2.location.as_deref(), Some("Studio B"));
    assert_eq!(v2.notes.as_deref(), Some("Spare"));
    assert!(
        count(
            &pool,
            &format!(
                "SELECT COUNT(*) AS n FROM sync_actions
                 WHERE event_type = 'asset.updated' AND aggregate_id = '{}'",
                v1.id
            ),
        ) > updates_before,
        "the restore's update emits"
    );

    let source_job = get_job(&pool, &ws, &source).await;
    assert_eq!(source_job["restored_by"]["id"], id.as_str(), "{source_job}");
    let restore_job = get_job(&pool, &ws, &id).await;
    assert_eq!(
        restore_job["restores"]["id"],
        source.as_str(),
        "{restore_job}"
    );
    assert_eq!(
        restore_job["restores"]["at"], source_job["completed_at"],
        "{restore_job}"
    );
    assert!(restore_job.get("restored_by").is_none(), "{restore_job}");

    // The restore's answers stay on the job; none is saved for the next
    // import.
    assert_eq!(
        count(
            &pool,
            &format!("SELECT COUNT(*) AS n FROM import_value_answers WHERE job_id = '{id}'"),
        ),
        0
    );
}

/// A value edited since the import is left out and counted on Review, and
/// stays as it was edited; the draft's notice survives Check again.
#[actix_web::test]
async fn a_value_changed_since_is_left_as_it_is() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let source = update_import(&pool, &ws).await;
    exec(
        &pool,
        "UPDATE assets SET location = 'Desk' WHERE asset_tag = 'V-2'",
    );

    let (status, draft) = restore(&pool, &ws, &source).await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    assert_eq!(notice(&draft, "restore_changed_since"), Some(1), "{draft}");
    let id = id_of(&draft);
    let planned = rows(&pool, &ws, &id).await;
    let v2 = planned.iter().find(|r| r["key"] == "V-2").expect("V-2 row");
    assert!(v2["fields_written"].get("location").is_none(), "{v2}");
    assert_eq!(
        v2["fields_written"]["notes"],
        serde_json::json!({ "before": "Loaned", "after": "Spare" }),
        "{v2}"
    );

    // Back to Map and Check again keeps the notice.
    let (status, again) = call(
        &pool,
        &ws,
        ws.admin_uuid,
        crate::imports_assets::mapping_request(
            &id,
            serde_json::json!({ "mapping": draft["mapping"], "options": draft["options"] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(notice(&again, "restore_changed_since"), Some(1), "{again}");

    commit(&pool, &ws, &id).await;
    let v2 = asset(&pool, "V-2");
    assert_eq!(v2.location.as_deref(), Some("Desk"));
    assert_eq!(v2.notes.as_deref(), Some("Spare"));
    assert_eq!(asset(&pool, "V-1").location.as_deref(), Some("Studio A"));
}

/// A second press while the draft is open answers the same draft; once it
/// is committed, every value is as it was and there is nothing to restore.
#[actix_web::test]
async fn a_second_press_returns_the_open_draft() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let source = update_import(&pool, &ws).await;

    let (status, first) = restore(&pool, &ws, &source).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let (status, second) = restore(&pool, &ws, &source).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["id"], first["id"]);
    assert_eq!(
        count(
            &pool,
            &format!("SELECT COUNT(*) AS n FROM import_jobs WHERE restore_of = '{source}'"),
        ),
        1
    );

    commit(&pool, &ws, &id_of(&first)).await;
    let (status, body) = restore(&pool, &ws, &source).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_nothing_to_restore", "{body}");
}

/// Users imports 400, a pruned import 410, an import not committed 409,
/// one whose values all changed since 409, another workspace 404 and a
/// member 403.
#[actix_web::test]
async fn refusals() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let (ws, other) = (&seeded.a, &seeded.b);
    let source = update_import(&pool, ws).await;

    let (status, body) = call(
        &pool,
        ws,
        ws.member_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{source}/restore")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = restore(&pool, other, &source).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let users = Uuid::new_v4();
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "INSERT INTO import_jobs (id, job_type, status, filename, file_path, created_by, completed_at, row_results)
             VALUES ($1, 'users', 'done', 'people.csv', '', $2, now(), '{\"phase\":\"committed\",\"rows\":[]}')",
        )
        .bind::<SqlUuid, _>(users)
        .bind::<SqlUuid, _>(ws.admin_uuid)
        .execute(c)
    })
    .expect("users job");
    let (status, body) = restore(&pool, ws, &users.to_string()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "import_previous_values_assets_only", "{body}");

    let draft = checked(
        &pool,
        ws,
        "Name,Tag,Location,Notes,Status\nV-1,V-1,Elsewhere,,\n",
        MAPPING,
        serde_json::json!({ "default_kind": "generic" }),
    )
    .await;
    let (status, body) = restore(&pool, ws, &id_of(&draft)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_not_committed", "{body}");

    exec(
        &pool,
        "UPDATE assets SET location = 'Desk', notes = 'Edited', status = 'in_service'
         WHERE asset_tag IN ('V-1', 'V-2')",
    );
    let (status, body) = restore(&pool, ws, &source).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_nothing_to_restore", "{body}");

    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("UPDATE import_jobs SET pruned_at = now() WHERE id = $1")
            .bind::<SqlUuid, _>(Uuid::parse_str(&source).expect("uuid"))
            .execute(c)
    })
    .expect("prune");
    let (status, body) = restore(&pool, ws, &source).await;
    assert_eq!(status, StatusCode::GONE, "{body}");
    assert_eq!(
        count(
            &pool,
            &format!("SELECT COUNT(*) AS n FROM import_jobs WHERE restore_of = '{source}'"),
        ),
        0
    );
}
