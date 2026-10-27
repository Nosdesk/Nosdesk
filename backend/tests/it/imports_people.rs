//! A primary user column on Match values: matched to a member by primary
//! email, an unknown email left unassigned (and counted) unless the admin
//! links it to a member or skips its rows; and a job stored with the
//! retired `create_missing` option still loads.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::Text;

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{call_import_route, json};
use crate::imports_attributes::{checked, commit, errors, id_of, member_email, workspace};

const MAPPING: &[(&str, &str)] = &[
    ("Name", "name"),
    ("Type", "kind"),
    ("Tag", "asset_tag"),
    ("Assigned To", "primary_user"),
];

async fn values(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> serde_json::Value {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/values")),
    )
    .await;
    let view = json(&body);
    assert_eq!(status, StatusCode::OK, "{view}");
    view
}

async fn answer(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    id: &str,
    answers: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::put()
            .uri(&format!("/api/admin/import/{id}/values"))
            .set_json(serde_json::json!({ "answers": answers })),
    )
    .await;
    (status, json(&body))
}

fn people(view: &serde_json::Value) -> &serde_json::Value {
    view["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|c| c["field"] == "primary_user")
        .unwrap_or_else(|| panic!("no primary_user column in {view}"))
}

fn value<'a>(column: &'a serde_json::Value, text: &str) -> &'a serde_json::Value {
    column["values"]
        .as_array()
        .expect("values")
        .iter()
        .find(|v| v["text"] == text)
        .unwrap_or_else(|| panic!("no value '{text}' in {column}"))
}

fn primary_user(pool: &TestPool, tag: &str) -> Option<String> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
        user: Option<String>,
    }
    diesel::sql_query("SELECT primary_user_uuid::text AS user FROM assets WHERE asset_tag = $1")
        .bind::<Text, _>(tag)
        .get_result::<Row>(&mut pool.get().expect("conn"))
        .expect("read asset")
        .user
}

/// A member's primary email links in any case; an unknown email has no
/// match and is left unassigned, which isn't a row error. The members are
/// the column's choices, with their emails, and can't be created or kept
/// as text.
#[actix_web::test]
async fn a_person_matches_by_email_and_an_unknown_one_is_left_unassigned() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    let file = format!(
        "Name,Type,Tag,Assigned To\n\
         Cam 1,camera,V-1,{}\n\
         Cam 2,camera,V-2,crew@contractor.example\n\
         Cam 3,camera,V-3,crew@contractor.example\n",
        member_email(&ws).to_uppercase()
    );
    let job = checked(&pool, &ws, &file, MAPPING, serde_json::json!({})).await;
    assert_eq!(errors(&job), vec![], "{job}");
    assert_eq!(job["summary"]["would_create"], 3);
    assert_eq!(job["summary"]["dropped"][0]["column"], "primary_user");
    assert_eq!(job["summary"]["dropped"][0]["count"], 2);

    let id = id_of(&job);
    let view = values(&pool, &ws, &id).await;
    let column = people(&view);
    assert_eq!(column["lookup"], "person");
    assert_eq!(column["can_create"], false);
    let member = column["choices"]
        .as_array()
        .expect("choices")
        .iter()
        .find(|c| c["value"] == ws.member_uuid.to_string())
        .unwrap_or_else(|| panic!("the member isn't a choice: {column}"));
    assert_eq!(member["detail"], member_email(&ws));
    assert_eq!(column["matched"], 1);
    let crew = value(column, "crew@contractor.example");
    assert_eq!(crew["match"], "no_match");
    assert_eq!(
        crew["suggested"],
        serde_json::json!({ "action": "leave_blank" })
    );
    assert_eq!(crew["rows"], serde_json::json!([3, 4]));

    for refused in ["create", "keep_text"] {
        let (status, body) = answer(
            &pool,
            &ws,
            &id,
            serde_json::json!({ "primary_user": { "": {
                "crew@contractor.example": { "action": refused }
            } } }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}: {body}");
    }

    commit(&pool, &ws, &id).await;
    assert_eq!(primary_user(&pool, "V-1"), Some(ws.member_uuid.to_string()));
    assert_eq!(primary_user(&pool, "V-2"), None);
}

/// The admin links an unknown email to a member, or skips its rows.
#[actix_web::test]
async fn a_person_answer_links_a_member_or_skips_the_rows() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    let file = "Name,Type,Tag,Assigned To\n\
                Cam 1,camera,V-1,sam.old@tvs.example\n\
                Cam 2,camera,V-2,hire@lens.example\n";
    let job = checked(&pool, &ws, file, MAPPING, serde_json::json!({})).await;
    let id = id_of(&job);
    let (status, job) = answer(
        &pool,
        &ws,
        &id,
        serde_json::json!({ "primary_user": { "": {
            "Sam.Old@tvs.example": { "action": "match", "target": ws.member_uuid.to_string() },
            "hire@lens.example": { "action": "skip" }
        } } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(
        errors(&job),
        vec![(
            3,
            Some("primary_user".to_string()),
            "'hire@lens.example' is set to Skip these rows".to_string()
        )]
    );
    assert_eq!(job["summary"]["dropped"], serde_json::json!([]));

    let view = values(&pool, &ws, &id).await;
    let old = value(people(&view), "sam.old@tvs.example");
    assert_eq!(
        old["answer"],
        serde_json::json!({ "action": "match", "target": ws.member_uuid.to_string() })
    );

    commit(&pool, &ws, &id).await;
    assert_eq!(primary_user(&pool, "V-1"), Some(ws.member_uuid.to_string()));
    let skipped: i64 =
        diesel::sql_query("SELECT count(*) AS n FROM assets WHERE asset_tag = 'V-2'")
            .get_result::<Count>(&mut pool.get().expect("conn"))
            .expect("count")
            .n;
    assert_eq!(skipped, 0);
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    n: i64,
}

/// A member removed between the check and the commit no longer takes the
/// rows linked to them: the commit finds the plan changed and writes
/// nothing.
#[actix_web::test]
async fn a_person_who_left_after_the_check_changes_the_plan() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    let file = format!(
        "Name,Type,Tag,Assigned To\n\
         Cam 1,camera,V-1,{}\n",
        member_email(&ws)
    );
    let job = checked(&pool, &ws, &file, MAPPING, serde_json::json!({})).await;
    assert_eq!(job["summary"]["dropped"], serde_json::json!([]));
    let id = id_of(&job);
    diesel::sql_query("UPDATE workspace_members SET removed_at = now() WHERE user_uuid = $1")
        .bind::<diesel::sql_types::Uuid, _>(ws.member_uuid)
        .execute(&mut pool.get().expect("conn"))
        .expect("remove member");

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    let body = json(&body);
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_plan_changed", "{body}");
    let written: i64 =
        diesel::sql_query("SELECT count(*) AS n FROM assets WHERE asset_tag = 'V-1'")
            .get_result::<Count>(&mut pool.get().expect("conn"))
            .expect("count")
            .n;
    assert_eq!(written, 0);
}

/// Options stored with `create_missing`, which nothing reads now, still
/// load: the job's values read and it commits. A request that sends it is
/// refused.
#[actix_web::test]
async fn a_job_stored_with_create_missing_still_loads() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    let file = "Name,Type,Tag,Assigned To\n\
                Cam 1,camera,V-1,crew@contractor.example\n";
    let job = checked(&pool, &ws, file, MAPPING, serde_json::json!({})).await;
    let id = id_of(&job);
    diesel::sql_query(
        "UPDATE import_jobs SET options = options || '{\"create_missing\": true}'::jsonb
         WHERE id = $1::uuid",
    )
    .bind::<Text, _>(&id)
    .execute(&mut pool.get().expect("conn"))
    .expect("store legacy option");

    let view = values(&pool, &ws, &id).await;
    assert_eq!(people(&view)["lookup"], "person");
    commit(&pool, &ws, &id).await;
    assert_eq!(primary_user(&pool, "V-1"), None);

    // On a new draft, the same mapping is taken without it and refused
    // with it.
    let draft = id_of(&checked(&pool, &ws, file, MAPPING, serde_json::json!({})).await);
    let remap = |options: serde_json::Value| {
        crate::imports_assets::mapping_request(
            &draft,
            serde_json::json!({
                "mapping": { "name": 0, "kind": 1, "asset_tag": 2, "primary_user": 3 },
                "options": options,
            }),
        )
    };
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        remap(serde_json::json!({ "create_missing": true })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{}", json(&body));
    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, remap(serde_json::json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{}", json(&body));
}
