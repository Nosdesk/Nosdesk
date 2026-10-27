//! Saved matches: the answers a commit keeps for the next import of the
//! type, read back on Match values as Saved match, listed on the Saved
//! matches page (`GET /admin/import/answers`), deleted one at a time or all
//! at once, and forgotten with the person they name.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Text};
use uuid::Uuid;

use backend::models::NewImportValueAnswer;
use backend::repository::imports as repo;
use backend::sync::actor::ActorContext;
use backend::sync::session::{pin_workspace, run_in_workspace, with_actor_bypass_context};

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{call_import_route, import_workspaces, json};
use crate::imports_attributes::{checked, commit, id_of, seed_kind};

const REF: &str = "test:imports_saved_matches";

const HEADER: &str = "name,kind,asset_tag,manufacturer,status,primary_user,quantity";
const MAPPING: &[(&str, &str)] = &[
    ("name", "name"),
    ("kind", "kind"),
    ("asset_tag", "asset_tag"),
    ("manufacturer", "manufacturer"),
    ("status", "status"),
    ("primary_user", "primary_user"),
    ("quantity", "quantity"),
];

/// Cam is a Guessed kind and tripod none; Blackmagic is a manufacturer
/// nothing matches and Sony Corp a Guessed one; Spare and In use are
/// Guessed statuses; the email is nobody's; "two" doesn't read.
fn file(tag_prefix: &str) -> String {
    format!(
        "{HEADER}\n\
         Cam A,Cam,{tag_prefix}-1,Blackmagic,Spare,nobody@example.com,two\n\
         Cam B,camera,{tag_prefix}-2,Sony Corp,In use,,1\n\
         Tripod,tripod,{tag_prefix}-3,,,,1\n"
    )
}

fn seed(pool: &TestPool, ws: &WorkspaceSeed) {
    seed_kind(
        pool,
        ws,
        "camera",
        "Camera",
        300,
        serde_json::json!({ "type": "object", "properties": {} }),
    );
    diesel::sql_query("INSERT INTO manufacturers (name, workspace_id) VALUES ('Sony', $1)")
        .bind::<Integer, _>(ws.workspace_id)
        .execute(&mut pool.get().expect("conn"))
        .expect("seed maker");
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn saved_count(pool: &TestPool, ws: &WorkspaceSeed) -> i64 {
    diesel::sql_query("SELECT COUNT(*) AS n FROM import_value_answers WHERE workspace_id = $1")
        .bind::<Integer, _>(ws.workspace_id)
        .get_result::<Count>(&mut pool.get().expect("conn"))
        .expect("count")
        .n
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = Integer)]
    id: i32,
}

fn maker_id(pool: &TestPool, ws: &WorkspaceSeed, name: &str) -> i32 {
    diesel::sql_query("SELECT id FROM manufacturers WHERE name = $1 AND workspace_id = $2")
        .bind::<Text, _>(name)
        .bind::<Integer, _>(ws.workspace_id)
        .get_result::<IdRow>(&mut pool.get().expect("conn"))
        .expect("maker")
        .id
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

async fn list(pool: &TestPool, ws: &WorkspaceSeed) -> serde_json::Value {
    let (status, view) = call(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import/answers?type=assets"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    view
}

fn entries(view: &serde_json::Value) -> &Vec<serde_json::Value> {
    view["entries"].as_array().expect("entries")
}

/// Each entry as (field, text, outcome, label).
fn summary(view: &serde_json::Value) -> Vec<(String, String, String, Option<String>)> {
    entries(view)
        .iter()
        .map(|e| {
            (
                e["field"].as_str().expect("field").to_string(),
                e["text"].as_str().expect("text").to_string(),
                e["outcome"].as_str().expect("outcome").to_string(),
                e["label"].as_str().map(String::from),
            )
        })
        .collect()
}

fn entry<'a>(view: &'a serde_json::Value, field: &str, text: &str) -> &'a serde_json::Value {
    entries(view)
        .iter()
        .find(|e| e["field"] == field && e["text"] == text)
        .unwrap_or_else(|| panic!("no {field} {text} in {view}"))
}

async fn put_values(pool: &TestPool, ws: &WorkspaceSeed, id: &str, answers: serde_json::Value) {
    let (status, job) = call(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::put()
            .uri(&format!("/api/admin/import/{id}/values"))
            .set_json(serde_json::json!({ "answers": answers })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
}

async fn values(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> serde_json::Value {
    let (status, view) = call(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/values")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    view
}

fn value<'a>(view: &'a serde_json::Value, field: &str, text: &str) -> &'a serde_json::Value {
    view["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .filter(|c| c["field"] == field)
        .flat_map(|c| c["values"].as_array().expect("values").iter())
        .find(|v| v["text"] == text)
        .unwrap_or_else(|| panic!("no {field} value {text} in {view}"))
}

/// The answers the first import gives: tripod skipped, Blackmagic created,
/// "two" replaced with 2. Everything else is left as matching suggests.
fn first_answers() -> serde_json::Value {
    serde_json::json!({
        "kind": { "": { "tripod": { "action": "skip" } } },
        "manufacturer": { "": { "blackmagic": { "action": "create" } } },
        "quantity": { "": { "two": { "action": "replace", "value": "2" } } },
    })
}

/// Answers are saved by the commit and not by a check. A Guessed value,
/// a discard left at its default, and a Create are saved; Skip, Replace
/// with and an exact match aren't. The Create is saved as a link to the
/// manufacturer the commit made. The next import shows each as a Saved
/// match with who saved it, a discard still listed. A later answer
/// replaces an earlier one, and a saved match nobody changed keeps the
/// import that saved it. The page lists them in field order with the
/// header memory.
#[actix_web::test]
async fn answers_are_saved_on_import_and_read_back() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    seed(&pool, ws);

    let job = checked(&pool, ws, &file("A"), MAPPING, serde_json::json!({})).await;
    let first = id_of(&job);
    put_values(&pool, ws, &first, first_answers()).await;
    assert_eq!(saved_count(&pool, ws), 0, "a check saves nothing");

    commit(&pool, ws, &first).await;
    let view = list(&pool, ws).await;
    let blackmagic = maker_id(&pool, ws, "Blackmagic");
    let sony = maker_id(&pool, ws, "Sony");
    let expect = |f: &str, t: &str, o: &str, l: Option<&str>| {
        (
            f.to_string(),
            t.to_string(),
            o.to_string(),
            l.map(String::from),
        )
    };
    assert_eq!(
        summary(&view),
        vec![
            expect("kind", "Cam", "link", Some("Camera")),
            expect("status", "In use", "link", Some("In service")),
            expect("status", "Spare", "link", Some("In stock")),
            expect(
                "primary_user",
                "nobody@example.com",
                "left_unassigned",
                None
            ),
            expect("manufacturer", "Blackmagic", "link", Some("Blackmagic")),
            expect("manufacturer", "Sony Corp", "link", Some("Sony")),
        ],
        "{view}"
    );
    let made = entry(&view, "manufacturer", "Blackmagic");
    assert_eq!(made["created"], true, "{made}");
    assert_eq!(made["target"], blackmagic.to_string());
    assert_eq!(
        entry(&view, "manufacturer", "Sony Corp")["target"],
        sony.to_string()
    );
    assert_eq!(entry(&view, "manufacturer", "Sony Corp")["created"], false);
    let cam = entry(&view, "kind", "Cam");
    assert_eq!(cam["saved"]["by"], ws.admin_uuid.to_string());
    assert!(cam["saved"]["by_name"].is_string(), "{cam}");
    assert_eq!(cam["saved"]["job_id"], first);
    assert_eq!(cam["field_label"], "Kind");
    let columns = &view["columns"];
    assert_eq!(columns["job_id"], first, "{columns}");
    let headers: Vec<(&str, Option<&str>)> = columns["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| (r["header"].as_str().expect("header"), r["field"].as_str()))
        .collect();
    assert_eq!(headers[1], ("kind", Some("kind")));
    assert_eq!(headers.len(), 7);

    // The next import reads them back.
    let job = checked(&pool, ws, &file("B"), MAPPING, serde_json::json!({})).await;
    let second = id_of(&job);
    let view = values(&pool, ws, &second).await;
    let cam = value(&view, "kind", "Cam");
    assert_eq!(cam["match"], "saved", "{cam}");
    assert_eq!(cam["saved"]["by"], ws.admin_uuid.to_string());
    assert_eq!(value(&view, "status", "Spare")["match"], "saved");
    let made = value(&view, "manufacturer", "Blackmagic");
    assert_eq!(
        made["suggested"],
        serde_json::json!({ "action": "match", "target": blackmagic.to_string() }),
        "{made}"
    );
    let corp = value(&view, "manufacturer", "Sony Corp");
    assert_eq!(corp["match"], "saved", "{corp}");
    let nobody = value(&view, "primary_user", "nobody@example.com");
    assert_eq!(nobody["match"], "saved", "{nobody}");
    assert_eq!(
        nobody["suggested"],
        serde_json::json!({ "action": "leave_blank" })
    );
    // Skip and Replace with ask again.
    assert_eq!(value(&view, "kind", "tripod")["match"], "no_match");
    assert_eq!(value(&view, "quantity", "two")["match"], "no_match");

    // A new answer replaces the saved one; the rest keep their import.
    put_values(
        &pool,
        ws,
        &second,
        serde_json::json!({
            "kind": { "": { "tripod": { "action": "skip" } } },
            "status": { "": { "spare": { "action": "match", "target": "in_repair" } } },
            "quantity": { "": { "two": { "action": "replace", "value": "2" } } },
        }),
    )
    .await;
    commit(&pool, ws, &second).await;
    let view = list(&pool, ws).await;
    assert_eq!(entries(&view).len(), 6, "{view}");
    let spare = entry(&view, "status", "Spare");
    assert_eq!(spare["label"], "In repair", "{spare}");
    assert_eq!(spare["saved"]["job_id"], second);
    assert_eq!(entry(&view, "kind", "Cam")["saved"]["job_id"], first);
    assert_eq!(view["columns"]["job_id"], second);
}

/// Deleting one saved match makes the next import ask about its value
/// again; Delete all clears the type and keeps the header memory. Both
/// are admin only, and a workspace sees and deletes only its own.
#[actix_web::test]
async fn saved_matches_are_deleted_by_admins_of_their_workspace() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let (ws, other) = (&seeded.a, &seeded.b);
    seed(&pool, ws);

    let job = checked(&pool, ws, &file("A"), MAPPING, serde_json::json!({})).await;
    let first = id_of(&job);
    put_values(&pool, ws, &first, first_answers()).await;
    commit(&pool, ws, &first).await;
    let view = list(&pool, ws).await;
    let spare = entry(&view, "status", "Spare")["id"]
        .as_str()
        .expect("id")
        .to_string();

    // Not for a member.
    for req in [
        TestRequest::get().uri("/api/admin/import/answers?type=assets"),
        TestRequest::delete().uri(&format!("/api/admin/import/answers/{spare}")),
        TestRequest::delete().uri("/api/admin/import/answers?type=assets"),
    ] {
        let (status, body) = call(&pool, ws, ws.member_uuid, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    // Another workspace sees none and deletes none.
    let (status, theirs) = call(
        &pool,
        other,
        other.admin_uuid,
        TestRequest::get().uri("/api/admin/import/answers?type=assets"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{theirs}");
    assert_eq!(entries(&theirs).len(), 0, "{theirs}");
    assert!(theirs["columns"].is_null(), "{theirs}");
    let (status, _) = call(
        &pool,
        other,
        other.admin_uuid,
        TestRequest::delete().uri(&format!("/api/admin/import/answers/{spare}")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = call(
        &pool,
        other,
        other.admin_uuid,
        TestRequest::delete().uri("/api/admin/import/answers?type=assets"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["deleted"], 0);
    assert_eq!(saved_count(&pool, ws), 6);

    // One.
    let (status, _) = call(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::delete().uri(&format!("/api/admin/import/answers/{spare}")),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::delete().uri(&format!("/api/admin/import/answers/{spare}")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let view = list(&pool, ws).await;
    assert_eq!(entries(&view).len(), 5);
    let job = checked(&pool, ws, &file("B"), MAPPING, serde_json::json!({})).await;
    let next = values(&pool, ws, &id_of(&job)).await;
    assert_eq!(value(&next, "status", "Spare")["match"], "guessed");
    assert_eq!(value(&next, "kind", "Cam")["match"], "saved");

    // All.
    let (status, body) = call(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::delete().uri("/api/admin/import/answers?type=assets"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["deleted"], 5);
    let view = list(&pool, ws).await;
    assert_eq!(entries(&view).len(), 0);
    assert!(
        view["columns"]["rows"].is_array(),
        "the header memory stays"
    );
    let next = values(&pool, ws, &id_of(&job)).await;
    assert_eq!(value(&next, "kind", "Cam")["match"], "guessed");

    let (status, _) = call(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import/answers?type=nope"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

fn saved_row(
    field: &str,
    value: &str,
    answer: serde_json::Value,
    by: Option<Uuid>,
) -> NewImportValueAnswer {
    NewImportValueAnswer {
        job_type: "assets".to_string(),
        field: field.to_string(),
        context: String::new(),
        normalized_value: value.to_string(),
        answer,
        answered_by: by,
        job_id: None,
        source_text: Some(value.to_string()),
        created: false,
    }
}

/// Erasing a person keeps the answers they gave, saved by a former member,
/// and forgets the ones that name them: their email as the value in the
/// file, or them as the answer.
#[actix_web::test]
async fn erasure_forgets_the_person_in_saved_matches() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    seed(&pool, ws);
    let email = format!("leaver-{}@example.com", ws.slug);
    diesel::sql_query(
        "INSERT INTO user_emails (user_uuid, email, is_primary, is_verified) VALUES ($1, $2, true, true)",
    )
    .bind::<diesel::sql_types::Uuid, _>(ws.member_uuid)
    .bind::<Text, _>(email.to_uppercase())
    .execute(&mut pool.get().expect("conn"))
    .expect("seed email");

    let runtime = db.runtime_pool(2);
    run_in_workspace(&runtime, REF, ws.workspace_id, |c| {
        repo::upsert_value_answers(
            c,
            &[
                saved_row(
                    "kind",
                    "cam",
                    serde_json::json!({ "action": "match", "target": "camera" }),
                    Some(ws.member_uuid),
                ),
                saved_row(
                    "primary_user",
                    &email,
                    serde_json::json!({ "action": "leave_blank" }),
                    Some(ws.admin_uuid),
                ),
                saved_row(
                    "primary_user",
                    "sam",
                    serde_json::json!({ "action": "match", "target": ws.member_uuid.to_string() }),
                    Some(ws.admin_uuid),
                ),
            ],
        )
    })
    .expect("save");
    let view = list(&pool, ws).await;
    assert_eq!(entries(&view).len(), 3, "{view}");

    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        pin_workspace(c, ws.workspace_id)?;
        backend::repository::users::purge_user(&ws.member_uuid, c, None)
    })
    .expect("purge the member");

    let view = list(&pool, ws).await;
    assert_eq!(
        summary(&view),
        vec![(
            "kind".to_string(),
            "cam".to_string(),
            "link".to_string(),
            Some("Camera".to_string())
        )],
        "{view}"
    );
    let cam = &entries(&view)[0];
    assert!(cam["saved"]["by"].is_null(), "{cam}");
    assert!(cam["saved"]["by_name"].is_null(), "{cam}");
    assert_eq!(saved_count(&pool, ws), 1);
}
