//! Remove imported assets (`/admin/import/{id}/remove-created`) and
//! Download previous values (`/admin/import/{id}/previous-values`): the
//! untouched rule one check at a time, the deletes with their sync actions
//! and the job's audit entry, the catalog entries removed or archived, run
//! once, the 30-day window, the preview agreeing with the result, and the
//! previous values file and its import back.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Nullable, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_context};

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{call_import_route, import_workspace, import_workspaces, json};
use crate::imports_attributes::{checked, commit, id_of, seed_kind};

const REF: &str = "test:imports_remove";

const MAPPING: &[(&str, &str)] = &[("Name", "name"), ("Tag", "asset_tag")];

fn generic() -> serde_json::Value {
    serde_json::json!({ "default_kind": "generic" })
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = Integer)]
    id: i32,
}

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

fn asset_id(pool: &TestPool, tag: &str) -> Option<i32> {
    diesel::sql_query("SELECT id FROM assets WHERE asset_tag = $1")
        .bind::<Text, _>(tag)
        .get_result::<IdRow>(&mut pool.get().expect("conn"))
        .optional()
        .expect("read asset")
        .map(|r| r.id)
}

/// Import assets tagged `tags` (named "Cam {tag}") and commit: the job id.
async fn import(pool: &TestPool, ws: &WorkspaceSeed, tags: &[&str]) -> String {
    let mut file = "Name,Tag\n".to_string();
    for tag in tags {
        file.push_str(&format!("Cam {tag},{tag}\n"));
    }
    let job = checked(pool, ws, &file, MAPPING, generic()).await;
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

async fn preview(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> (StatusCode, serde_json::Value) {
    call(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/remove-created")),
    )
    .await
}

async fn remove(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> (StatusCode, serde_json::Value) {
    call(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/remove-created")),
    )
    .await
}

async fn previous_values(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> (StatusCode, String) {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/previous-values")),
    )
    .await;
    (status, String::from_utf8_lossy(&body).to_string())
}

/// The kept list as (tag, codes of its reasons).
fn kept(result: &serde_json::Value) -> Vec<(String, Vec<String>)> {
    result["kept"]
        .as_array()
        .expect("kept")
        .iter()
        .map(|k| {
            (
                k["tag"].as_str().unwrap_or_default().to_string(),
                k["reasons"]
                    .as_array()
                    .expect("reasons")
                    .iter()
                    .map(|r| r["code"].as_str().expect("code").to_string())
                    .collect(),
            )
        })
        .collect()
}

/// Run `sql` with `$1` bound to the job, pinned to its workspace so the
/// audit trigger has one.
fn on_job(pool: &TestPool, ws: &WorkspaceSeed, sql: &str, id: &str) {
    let id = Uuid::parse_str(id).expect("uuid");
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(sql).bind::<SqlUuid, _>(id).execute(c)
    })
    .expect(sql);
}

/// Import R-1 and R-2, touch R-1 with `touch`, then remove: R-1 is kept
/// with `code` as its first reason, R-2 is deleted, and the preview said
/// the same beforehand.
async fn kept_for(
    touch: impl FnOnce(&TestPool, &WorkspaceSeed, i32),
    code: &str,
) -> serde_json::Value {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id = import(&pool, &ws, &["R-1", "R-2"]).await;
    let touched = asset_id(&pool, "R-1").expect("R-1");
    touch(&pool, &ws, touched);

    let (status, plan) = preview(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan["created"], 2, "{plan}");
    assert_eq!(plan["deletable"], 1, "{plan}");
    let planned = kept(&plan);
    assert_eq!(planned.len(), 1, "{plan}");
    assert_eq!(planned[0].0, "R-1");
    assert_eq!(planned[0].1[0], code, "{plan}");

    let (status, job) = remove(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let result = &job["remove_result"];
    assert_eq!(kept(result), planned, "{result}");
    assert_eq!(result["deleted"], 1);
    assert_eq!(
        asset_id(&pool, "R-1"),
        Some(touched),
        "the touched asset stays"
    );
    assert_eq!(asset_id(&pool, "R-2"), None, "the untouched one goes");
    result["kept"][0]["reasons"][0].clone()
}

/// An edit after the import moves `updated_at`: "Edited on 3 Nov", with who
/// from the audit log.
#[actix_web::test]
async fn an_edited_asset_is_kept() {
    let reason = kept_for(
        |pool, ws, asset| {
            let actor = ActorContext::user(ws.member_uuid, None).with_workspace(ws.workspace_id);
            let mut conn = pool.get().expect("conn");
            with_actor_context(&mut conn, &actor, |c| {
                backend::repository::assets::update_device(
                    c,
                    asset,
                    backend::models::AssetUpdate {
                        location: Some(Some("Studio B".to_string())),
                        ..Default::default()
                    },
                )
            })
            .expect("edit the asset");
        },
        "edited",
    )
    .await;
    assert!(reason["at"].is_string(), "{reason}");
    assert!(
        reason["by_name"]
            .as_str()
            .is_some_and(|n| n.contains("Member")),
        "{reason}"
    );
}

/// A lifecycle event after the import's own: "Status changed since".
#[actix_web::test]
async fn an_asset_whose_status_moved_is_kept() {
    kept_for(
        |pool, ws, asset| {
            diesel::sql_query(
                "INSERT INTO asset_lifecycle_events (asset_id, from_status, to_status, reason, workspace_id)
                 VALUES ($1, 'in_service', 'in_repair', 'Repair', $2)",
            )
            .bind::<Integer, _>(asset)
            .bind::<Integer, _>(ws.workspace_id)
            .execute(&mut pool.get().expect("conn"))
            .expect("event");
        },
        "status_changed",
    )
    .await;
}

#[actix_web::test]
async fn an_asset_with_a_loan_is_kept() {
    kept_for(
        |pool, ws, asset| {
            diesel::sql_query(
                "INSERT INTO asset_loans (asset_id, borrower_user_uuid, status_before, returned_at, workspace_id)
                 VALUES ($1, $2, 'in_service', now(), $3)",
            )
            .bind::<Integer, _>(asset)
            .bind::<SqlUuid, _>(ws.member_uuid)
            .bind::<Integer, _>(ws.workspace_id)
            .execute(&mut pool.get().expect("conn"))
            .expect("loan");
        },
        "loan",
    )
    .await;
}

#[actix_web::test]
async fn a_disposed_asset_is_kept() {
    kept_for(
        |pool, ws, asset| {
            diesel::sql_query(
                "INSERT INTO asset_disposals (asset_id, sanitization_method, workspace_id)
                 VALUES ($1, 'destroy', $2)",
            )
            .bind::<Integer, _>(asset)
            .bind::<Integer, _>(ws.workspace_id)
            .execute(&mut pool.get().expect("conn"))
            .expect("disposal");
        },
        "disposed",
    )
    .await;
}

/// Photos and files: "Has 2 photos".
#[actix_web::test]
async fn an_asset_with_photos_is_kept() {
    let reason = kept_for(
        |pool, ws, asset| {
            for (name, kind) in [
                ("front.jpg", "photo"),
                ("back.jpg", "photo"),
                ("manual.pdf", "document"),
            ] {
                diesel::sql_query(
                    "INSERT INTO asset_media (asset_id, url, name, kind, workspace_id)
                     VALUES ($1, '/files/x', $2, $3, $4)",
                )
                .bind::<Integer, _>(asset)
                .bind::<Text, _>(name)
                .bind::<Text, _>(kind)
                .bind::<Integer, _>(ws.workspace_id)
                .execute(&mut pool.get().expect("conn"))
                .expect("media");
            }
        },
        "media",
    )
    .await;
    assert_eq!(reason["photos"], 2);
    assert_eq!(reason["files"], 1);
}

#[actix_web::test]
async fn an_audited_asset_is_kept() {
    let reason = kept_for(
        |pool, ws, asset| {
            diesel::sql_query(
                "INSERT INTO asset_audits (asset_id, counted_quantity, previous_quantity, delta, workspace_id)
                 VALUES ($1, 3, 3, 0, $2)",
            )
            .bind::<Integer, _>(asset)
            .bind::<Integer, _>(ws.workspace_id)
            .execute(&mut pool.get().expect("conn"))
            .expect("audit");
        },
        "audited",
    )
    .await;
    assert!(reason["at"].is_string(), "{reason}");
}

#[actix_web::test]
async fn an_asset_with_usage_logged_is_kept() {
    kept_for(
        |pool, ws, asset| {
            diesel::sql_query(
                "INSERT INTO asset_usage_log (asset_id, quantity_used, unit, event_kind, workspace_id)
                 VALUES ($1, 1, 'each', 'usage', $2)",
            )
            .bind::<Integer, _>(asset)
            .bind::<Integer, _>(ws.workspace_id)
            .execute(&mut pool.get().expect("conn"))
            .expect("usage");
        },
        "usage",
    )
    .await;
}

/// A group the import didn't add it to: "Added to a group since".
#[actix_web::test]
async fn an_asset_added_to_a_group_since_is_kept() {
    kept_for(
        |pool, ws, asset| {
            let group = diesel::sql_query(
                "INSERT INTO asset_groups (name, workspace_id) VALUES ('Later', $1) RETURNING id",
            )
            .bind::<Integer, _>(ws.workspace_id)
            .get_result::<IdRow>(&mut pool.get().expect("conn"))
            .expect("group")
            .id;
            diesel::sql_query(
                "INSERT INTO asset_group_assignments (group_id, asset_id, workspace_id) VALUES ($1, $2, $3)",
            )
            .bind::<Integer, _>(group)
            .bind::<Integer, _>(asset)
            .bind::<Integer, _>(ws.workspace_id)
            .execute(&mut pool.get().expect("conn"))
            .expect("assign");
        },
        "group_added",
    )
    .await;
}

/// "Linked to ticket #412".
#[actix_web::test]
async fn an_asset_linked_to_a_ticket_is_kept() {
    let mut ticket = 0;
    let reason = kept_for(
        |pool, ws, asset| {
            let mut conn = pool.get().expect("conn");
            let state = diesel::sql_query(
                "SELECT id FROM workflow_states WHERE workspace_id = $1 ORDER BY is_default DESC, id LIMIT 1",
            )
            .bind::<Integer, _>(ws.workspace_id)
            .get_result::<IdRow>(&mut conn)
            .expect("state")
            .id;
            ticket = diesel::sql_query(
                "INSERT INTO tickets (workspace_id, title, workflow_state_id) VALUES ($1, 'Lens cap', $2) RETURNING id",
            )
            .bind::<Integer, _>(ws.workspace_id)
            .bind::<Integer, _>(state)
            .get_result::<IdRow>(&mut conn)
            .expect("ticket")
            .id;
            diesel::sql_query(
                "INSERT INTO ticket_assets (ticket_id, asset_id, workspace_id) VALUES ($1, $2, $3)",
            )
            .bind::<Integer, _>(ticket)
            .bind::<Integer, _>(asset)
            .bind::<Integer, _>(ws.workspace_id)
            .execute(&mut conn)
            .expect("link");
        },
        "ticket",
    )
    .await;
    assert_eq!(reason["ticket_id"], ticket);
}

#[actix_web::test]
async fn an_asset_in_a_directory_group_is_kept() {
    kept_for(
        |pool, ws, asset| {
            let group = diesel::sql_query(
                "INSERT INTO groups (name, external_source, workspace_id) VALUES ('Entra cams', 'entra', $1) RETURNING id",
            )
            .bind::<Integer, _>(ws.workspace_id)
            .get_result::<IdRow>(&mut pool.get().expect("conn"))
            .expect("directory group")
            .id;
            diesel::sql_query(
                "INSERT INTO asset_directory_memberships (asset_id, group_id, workspace_id) VALUES ($1, $2, $3)",
            )
            .bind::<Integer, _>(asset)
            .bind::<Integer, _>(group)
            .bind::<Integer, _>(ws.workspace_id)
            .execute(&mut pool.get().expect("conn"))
            .expect("membership");
        },
        "directory",
    )
    .await;
}

/// A sync source took it over: "Now synced from Intune", ahead of the edit
/// the takeover also is.
#[actix_web::test]
async fn an_asset_now_synced_is_kept() {
    let reason = kept_for(
        |pool, _ws, asset| {
            diesel::sql_query("UPDATE assets SET external_sync_source = 'intune' WHERE id = $1")
                .bind::<Integer, _>(asset)
                .execute(&mut pool.get().expect("conn"))
                .expect("sync");
        },
        "synced",
    )
    .await;
    assert_eq!(reason["source"], "intune");
}

/// Another asset's asset-reference attribute names it: "Referenced by
/// 'Tripod kit 4'".
#[actix_web::test]
async fn an_asset_another_one_references_is_kept() {
    let reason = kept_for(
        |pool, ws, asset| {
            seed_kind(
                pool,
                ws,
                "kit",
                "Kit",
                320,
                serde_json::json!({
                    "type": "object",
                    "properties": { "camera": { "type": "string", "format": "asset-ref", "title": "Camera" } }
                }),
            );
            diesel::sql_query(
                "INSERT INTO assets (name, kind, attributes, workspace_id) VALUES ('Tripod kit 4', 'kit', $1::jsonb, $2)",
            )
            .bind::<Text, _>(serde_json::json!({ "camera": asset.to_string() }).to_string())
            .bind::<Integer, _>(ws.workspace_id)
            .execute(&mut pool.get().expect("conn"))
            .expect("referrer");
        },
        "referenced",
    )
    .await;
    assert_eq!(reason["name"], "Tripod kit 4");
}

/// The untouched assets are deleted through `delete_device`: each leaves
/// an `asset.deleted` sync action and an audit row, and the job's own audit
/// entry carries the result with its kept list. The job records when and
/// by whom, Recent imports counts it, and `updated_at` stays.
#[actix_web::test]
async fn untouched_assets_are_deleted_with_sync_actions_and_an_audit_entry() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id = import(&pool, &ws, &["D-1", "D-2", "D-3"]).await;
    let ids: Vec<i32> = ["D-1", "D-2", "D-3"]
        .iter()
        .map(|t| asset_id(&pool, t).expect("imported"))
        .collect();
    exec(
        &pool,
        &format!(
            "INSERT INTO asset_usage_log (asset_id, quantity_used, unit, event_kind, workspace_id)
             VALUES ({}, 1, 'each', 'usage', {})",
            ids[2], ws.workspace_id
        ),
    );
    let before: chrono::DateTime<chrono::Utc> = crate::imports_assets::json(
        &call_import_route(
            &pool,
            &ws,
            ws.admin_uuid,
            TestRequest::get().uri(&format!("/api/admin/import/{id}")),
        )
        .await
        .1,
    )["updated_at"]
        .as_str()
        .expect("updated_at")
        .parse()
        .expect("time");

    let (status, job) = remove(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert!(job["removed_at"].is_string(), "{job}");
    assert_eq!(job["removed_by"], ws.admin_uuid.to_string());
    assert!(
        job["removed_by_name"]
            .as_str()
            .is_some_and(|n| n.contains("Admin")),
        "{job}"
    );
    let result = &job["remove_result"];
    assert_eq!(result["created"], 3);
    assert_eq!(result["deleted"], 2);
    assert_eq!(result["deleted_ids"], serde_json::json!([ids[0], ids[1]]));
    assert_eq!(
        kept(result),
        vec![("D-3".to_string(), vec!["usage".to_string()])]
    );
    assert_eq!(
        job["updated_at"]
            .as_str()
            .expect("updated_at")
            .parse::<chrono::DateTime<chrono::Utc>>()
            .expect("time"),
        before,
        "the job keeps its place in header memory"
    );

    for gone in &ids[..2] {
        assert_eq!(
            count(
                &pool,
                &format!(
                    "SELECT COUNT(*) AS n FROM sync_actions
                     WHERE event_type = 'asset.deleted' AND aggregate_id = '{gone}'"
                ),
            ),
            1,
            "sync action for asset {gone}"
        );
        assert_eq!(
            count(
                &pool,
                &format!(
                    "SELECT COUNT(*) AS n FROM audit_log
                     WHERE table_name = 'assets' AND op = 'D' AND pk_text = '{gone}'"
                ),
            ),
            1,
            "audit row for asset {gone}"
        );
    }

    #[derive(QueryableByName)]
    struct Entry {
        #[diesel(sql_type = Nullable<diesel::sql_types::Jsonb>)]
        after_jsonb: Option<serde_json::Value>,
        #[diesel(sql_type = Nullable<SqlUuid>)]
        actor_uuid: Option<Uuid>,
    }
    let entries: Vec<Entry> = diesel::sql_query(
        "SELECT after_jsonb, actor_uuid FROM audit_log
         WHERE table_name = 'import_jobs' AND op = 'U' AND pk_text = $1
           AND 'remove_result' = ANY(changed_cols)",
    )
    .bind::<Text, _>(&id)
    .load(&mut pool.get().expect("conn"))
    .expect("audit entries");
    assert_eq!(entries.len(), 1, "one audit entry for the removal");
    let after = entries[0].after_jsonb.clone().expect("after");
    assert_eq!(after["remove_result"]["kept"][0]["tag"], "D-3", "{after}");
    assert_eq!(after["remove_result"]["deleted"], 2);
    assert_eq!(entries[0].actor_uuid, Some(ws.admin_uuid));

    let (status, list) = call(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        list["jobs"][0]["removed"],
        serde_json::json!({ "deleted": 2, "created": 3, "kept": 1 })
    );
}

const CATALOG_MAPPING: &[(&str, &str)] = &[
    ("Name", "name"),
    ("Tag", "asset_tag"),
    ("Make", "manufacturer"),
    ("Model", "model"),
    ("Groups", "groups"),
];

/// Import C-1 and C-2 creating the manufacturer Blackmagic, its model URSA
/// Mini and the groups New Room and Spare Room: the job id.
async fn catalog_import(pool: &TestPool, ws: &WorkspaceSeed) -> String {
    let file = "Name,Tag,Make,Model,Groups\n\
                Cam 1,C-1,Blackmagic,URSA Mini,New Room;Spare Room\n\
                Cam 2,C-2,Blackmagic,URSA Mini,New Room\n";
    let job = checked(pool, ws, file, CATALOG_MAPPING, generic()).await;
    let id = id_of(&job);
    let create = serde_json::json!({ "action": "create" });
    let (status, body) = call(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::put()
            .uri(&format!("/api/admin/import/{id}/values"))
            .set_json(serde_json::json!({ "answers": {
                "manufacturer": { "": { "Blackmagic": create } },
                "model": { "blackmagic": { "URSA Mini": create } },
                "groups": { "": { "New Room": create, "Spare Room": create } }
            } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["summary"]["would_create"], 2, "{body}");
    commit(pool, ws, &id).await;
    id
}

fn catalog_count(pool: &TestPool, table: &str, name: &str) -> i64 {
    count(
        pool,
        &format!("SELECT COUNT(*) AS n FROM {table} WHERE name = '{name}'"),
    )
}

/// With every asset gone, the manufacturer, model and groups the import
/// created go too, except one changed since.
#[actix_web::test]
async fn created_catalog_entries_go_when_nothing_uses_them() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id = catalog_import(&pool, &ws).await;
    exec(
        &pool,
        "UPDATE asset_groups SET description = 'Renamed' WHERE name = 'Spare Room'",
    );

    let (status, plan) = preview(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(
        plan["catalog"],
        serde_json::json!({ "manufacturers": 1, "models": 1, "groups": 1 })
    );

    let (status, job) = remove(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let catalog = &job["remove_result"]["catalog"];
    assert_eq!(
        catalog["manufacturers"]["deleted"].as_array().map(Vec::len),
        Some(1),
        "{catalog}"
    );
    assert_eq!(
        catalog["models"]["deleted"].as_array().map(Vec::len),
        Some(1),
        "{catalog}"
    );
    assert_eq!(
        catalog["groups"]["deleted"].as_array().map(Vec::len),
        Some(1),
        "{catalog}"
    );
    assert_eq!(
        catalog["groups"]["kept"].as_array().map(Vec::len),
        Some(1),
        "{catalog}"
    );
    assert_eq!(catalog_count(&pool, "manufacturers", "Blackmagic"), 0);
    assert_eq!(catalog_count(&pool, "asset_models", "URSA Mini"), 0);
    assert_eq!(catalog_count(&pool, "asset_groups", "New Room"), 0);
    assert_eq!(catalog_count(&pool, "asset_groups", "Spare Room"), 1);
}

/// An asset that stays keeps its model, and so the manufacturer; a created
/// group still holding it is archived, not deleted.
#[actix_web::test]
async fn a_used_model_stays_and_a_group_still_assigned_is_archived() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id = catalog_import(&pool, &ws).await;
    let c2 = asset_id(&pool, "C-2").expect("C-2");
    exec(
        &pool,
        &format!("UPDATE assets SET location = 'Truck' WHERE id = {c2}"),
    );

    let (status, job) = remove(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let result = &job["remove_result"];
    assert_eq!(
        kept(result),
        vec![("C-2".to_string(), vec!["edited".to_string()])]
    );
    let catalog = &result["catalog"];
    assert_eq!(
        catalog["models"]["kept"].as_array().map(Vec::len),
        Some(1),
        "{catalog}"
    );
    assert_eq!(
        catalog["manufacturers"]["kept"].as_array().map(Vec::len),
        Some(1),
        "{catalog}"
    );
    assert_eq!(
        catalog["groups"]["archived"].as_array().map(Vec::len),
        Some(1),
        "{catalog}"
    );
    assert_eq!(
        catalog["groups"]["deleted"].as_array().map(Vec::len),
        Some(1),
        "{catalog}"
    );
    assert_eq!(catalog_count(&pool, "asset_models", "URSA Mini"), 1);
    assert_eq!(catalog_count(&pool, "manufacturers", "Blackmagic"), 1);
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) AS n FROM asset_groups WHERE name = 'New Room' AND archived_at IS NOT NULL"
        ),
        1
    );
    assert_eq!(catalog_count(&pool, "asset_groups", "Spare Room"), 0);
    assert_eq!(
        count(
            &pool,
            &format!("SELECT COUNT(*) AS n FROM asset_group_assignments WHERE asset_id = {c2}")
        ),
        1,
        "the kept asset keeps its group"
    );
}

/// It runs once: a second removal, and its preview, are refused, and
/// nothing more is deleted. With everything kept there is nothing to
/// remove, and that doesn't use up the one run.
#[actix_web::test]
async fn removal_runs_once() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id = import(&pool, &ws, &["O-1", "O-2"]).await;
    let o1 = asset_id(&pool, "O-1").expect("O-1");
    let o2 = asset_id(&pool, "O-2").expect("O-2");
    for asset in [o1, o2] {
        exec(
            &pool,
            &format!("UPDATE assets SET notes = 'mine' WHERE id = {asset}"),
        );
    }
    let (status, body) = remove(&pool, &ws, &id).await;
    assert_nothing_to_remove(
        status,
        &body,
        "all_changed",
        "Every asset this import created has been changed since, so nothing was deleted.",
    );

    let id = import(&pool, &ws, &["O-3"]).await;
    let (status, body) = remove(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id_again = import(&pool, &ws, &["O-3"]).await;
    let (status, body) = remove(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_already_removed", "{body}");
    let (status, body) = preview(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_already_removed", "{body}");
    assert!(
        asset_id(&pool, "O-3").is_some(),
        "the second import's asset stays"
    );
    let (status, _) = preview(&pool, &ws, &id_again).await;
    assert_eq!(status, StatusCode::OK);
}

fn assert_nothing_to_remove(
    status: StatusCode,
    body: &serde_json::Value,
    reason: &str,
    message: &str,
) {
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_nothing_to_remove", "{body}");
    assert_eq!(body["args"]["reason"], reason, "{body}");
    assert_eq!(body["error"], message, "{body}");
}

/// An import that only updated assets created none: the preview says so,
/// and removal is refused with that reason, not "changed since".
#[actix_web::test]
async fn an_import_that_created_nothing_has_nothing_to_remove() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed(&pool, &ws, "N-1", "Studio A");
    let job = checked(
        &pool,
        &ws,
        "Tag,Location\nN-1,Studio B\n",
        &[("Tag", "asset_tag"), ("Location", "location")],
        serde_json::json!({ "mode": "update_only" }),
    )
    .await;
    assert_eq!(job["summary"]["would_create"], 0, "{job}");
    let id = id_of(&job);
    commit(&pool, &ws, &id).await;

    let (status, plan) = preview(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(
        (&plan["created"], &plan["deletable"]),
        (&0.into(), &0.into())
    );

    let (status, body) = remove(&pool, &ws, &id).await;
    assert_nothing_to_remove(
        status,
        &body,
        "none_created",
        "This import created no assets, so there is nothing to remove.",
    );
    assert!(asset_id(&pool, "N-1").is_some(), "the updated asset stays");
}

/// Once every asset an import created was deleted some other way, removal
/// is refused as already deleted.
#[actix_web::test]
async fn removal_after_the_created_assets_were_deleted_says_so() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id = import(&pool, &ws, &["D-1", "D-2"]).await;
    for tag in ["D-1", "D-2"] {
        let asset = asset_id(&pool, tag).expect(tag);
        run_in_workspace(&pool, REF, ws.workspace_id, |c| {
            backend::repository::assets::delete_device(c, asset, None).map(|_| ())
        })
        .expect("delete the asset");
    }

    let (status, plan) = preview(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!((&plan["created"], &plan["gone"]), (&2.into(), &2.into()));

    let (status, body) = remove(&pool, &ws, &id).await;
    assert_nothing_to_remove(
        status,
        &body,
        "all_gone",
        "The assets this import created were already deleted.",
    );
}

/// Thirty days after the commit removal is refused with its own code and
/// deletes nothing; once the prune drops the row results it is still
/// refused, and the previous values answer 410.
#[actix_web::test]
async fn removal_closes_thirty_days_after_the_commit() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id = import(&pool, &ws, &["W-1"]).await;
    let (status, job) = call(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let completed: chrono::DateTime<chrono::Utc> = job["completed_at"]
        .as_str()
        .expect("completed")
        .parse()
        .expect("time");
    let closes: chrono::DateTime<chrono::Utc> = job["removal_closes_at"]
        .as_str()
        .expect("closes")
        .parse()
        .expect("time");
    assert_eq!(closes - completed, chrono::Duration::days(30));

    on_job(
        &pool,
        &ws,
        "UPDATE import_jobs SET completed_at = now() - interval '31 days' WHERE id = $1",
        &id,
    );
    for (status, body) in [
        preview(&pool, &ws, &id).await,
        remove(&pool, &ws, &id).await,
    ] {
        assert_eq!(status, StatusCode::GONE, "{body}");
        assert_eq!(body["code"], "import_removal_window_closed", "{body}");
    }
    assert!(asset_id(&pool, "W-1").is_some());
    let (status, _) = previous_values(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "kept until the prune");

    let cutoff = chrono::Utc::now() - backend::services::imports::history::details_window();
    backend::services::scheduled_jobs::prune_import_history_before(&db.job_pool(), cutoff)
        .await
        .expect("prune");
    on_job(
        &pool,
        &ws,
        "UPDATE import_jobs SET completed_at = now() WHERE id = $1",
        &id,
    );
    let (status, body) = remove(&pool, &ws, &id).await;
    assert_eq!(
        status,
        StatusCode::GONE,
        "pruned rows never run the rule: {body}"
    );
    assert_eq!(body["code"], "import_removal_window_closed", "{body}");
    assert!(asset_id(&pool, "W-1").is_some());
    let (status, body) = previous_values(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::GONE, "{body}");
    assert!(body.contains("import_details_pruned"), "{body}");
}

/// A member gets 403 from every route; another workspace's admin gets 404.
#[actix_web::test]
async fn removal_and_previous_values_are_admin_only_and_workspace_scoped() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let (ws, other) = (&seeded.a, &seeded.b);
    let id = import(&pool, ws, &["S-1"]).await;
    let requests = || {
        [
            TestRequest::get().uri(&format!("/api/admin/import/{id}/remove-created")),
            TestRequest::post().uri(&format!("/api/admin/import/{id}/remove-created")),
            TestRequest::get().uri(&format!("/api/admin/import/{id}/previous-values")),
        ]
    };
    for req in requests() {
        let (status, _) = call(&pool, ws, ws.member_uuid, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    for req in requests() {
        let (status, body) = call(&pool, other, other.admin_uuid, req).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }
    assert!(asset_id(&pool, "S-1").is_some());
}

/// Users and tickets imports offer neither.
#[actix_web::test]
async fn only_asset_imports_offer_removal_and_previous_values() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id = Uuid::new_v4();
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "INSERT INTO import_jobs (id, job_type, status, filename, file_path, created_by, completed_at, row_results)
             VALUES ($1, 'users', 'done', 'people.csv', '', $2, now(), '{\"phase\":\"committed\",\"rows\":[]}')",
        )
        .bind::<SqlUuid, _>(id)
        .bind::<SqlUuid, _>(ws.admin_uuid)
        .execute(c)
    })
    .expect("users job");
    let id = id.to_string();
    let (status, body) = preview(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "import_removal_assets_only");
    let (status, body) = remove(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = previous_values(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.contains("import_previous_values_assets_only"),
        "{body}"
    );
    let (_, job) = call(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
    )
    .await;
    assert!(job["removal_closes_at"].is_null(), "{job}");
    assert!(job.get("previous_values").is_none(), "{job}");
}

/// The preview's counts and kept list are what the removal then does: an
/// asset deleted some other way counts as gone, not kept or deleted.
#[actix_web::test]
async fn the_preview_counts_match_the_result() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id = import(&pool, &ws, &["P-1", "P-2", "P-3", "P-4"]).await;
    let p1 = asset_id(&pool, "P-1").expect("P-1");
    let p4 = asset_id(&pool, "P-4").expect("P-4");
    exec(
        &pool,
        &format!(
            "INSERT INTO asset_media (asset_id, url, name, kind, workspace_id)
             VALUES ({p1}, '/files/x', 'a.jpg', 'photo', {})",
            ws.workspace_id
        ),
    );
    exec(&pool, &format!("DELETE FROM assets WHERE id = {p4}"));

    let (status, plan) = preview(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(
        (&plan["created"], &plan["deletable"], &plan["gone"]),
        (
            &serde_json::json!(4),
            &serde_json::json!(2),
            &serde_json::json!(1)
        ),
        "{plan}"
    );
    assert!(plan["closes_at"].is_string(), "{plan}");

    let (status, job) = remove(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let result = &job["remove_result"];
    assert_eq!(result["created"], plan["created"]);
    assert_eq!(result["deleted"], plan["deletable"]);
    assert_eq!(result["gone"], plan["gone"]);
    assert_eq!(result["kept"], plan["kept"]);
    assert_eq!(
        kept(result),
        vec![("P-1".to_string(), vec!["media".to_string()])]
    );
}

#[derive(QueryableByName, Debug, PartialEq)]
struct Restorable {
    #[diesel(sql_type = Nullable<Text>)]
    location: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    notes: Option<String>,
    #[diesel(sql_type = Text)]
    status: String,
    #[diesel(sql_type = Text)]
    name: String,
}

fn restorable(pool: &TestPool, tag: &str) -> Restorable {
    diesel::sql_query("SELECT location, notes, status, name FROM assets WHERE asset_tag = $1")
        .bind::<Text, _>(tag)
        .get_result(&mut pool.get().expect("conn"))
        .expect("read asset")
}

fn seed(pool: &TestPool, ws: &WorkspaceSeed, tag: &str, location: &str) {
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, location, status, workspace_id)
         VALUES ($1, 'generic', $1, $2, 'in_service', $3)",
    )
    .bind::<Text, _>(tag)
    .bind::<Text, _>(location)
    .bind::<Integer, _>(ws.workspace_id)
    .execute(&mut pool.get().expect("conn"))
    .expect("seed asset");
}

const UPDATE_MAPPING: &[(&str, &str)] = &[
    ("Name", "name"),
    ("Tag", "asset_tag"),
    ("Location", "location"),
    ("Notes", "notes"),
    ("Status", "status"),
];

/// Update V-1 and V-2 and create V-3: the job id.
async fn update_import(pool: &TestPool, ws: &WorkspaceSeed) -> String {
    seed(pool, ws, "V-1", "Studio A");
    seed(pool, ws, "V-2", "Studio B");
    let file = "Name,Tag,Location,Notes,Status\n\
                V-1,V-1,Field kit 2,Dented,in_repair\n\
                V-2,V-2,Field kit 2,,\n\
                V-3,V-3,Field kit 2,,\n";
    let job = checked(pool, ws, file, UPDATE_MAPPING, generic()).await;
    assert_eq!(job["summary"]["would_update"], 2, "{job}");
    let id = id_of(&job);
    commit(pool, ws, &id).await;
    id
}

/// The file: the key, then each field the updates wrote, holding the
/// value before; a field the row didn't write, or that was blank, is
/// blank. The detail page's counts agree.
#[actix_web::test]
async fn previous_values_hold_what_the_updates_replaced() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id = update_import(&pool, &ws).await;

    let (status, body) = previous_values(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let lines: Vec<&str> = body.trim_start_matches('\u{FEFF}').lines().collect();
    assert_eq!(
        lines,
        vec![
            "asset_tag,status,location,notes",
            "V-1,in_service,Studio A,",
            "V-2,,Studio B,",
        ]
    );
    let (_, job) = call(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
    )
    .await;
    assert_eq!(
        job["previous_values"],
        serde_json::json!({ "rows": 2, "blank_before": 1 }),
        "{job}"
    );
}

/// Importing the previous values file matches by tag, lands the rows as
/// Updated, and puts the values back; the field that was blank before
/// keeps the import's value. The file has no Name or Kind column, and Map
/// takes it as it is.
#[actix_web::test]
async fn previous_values_import_back_as_updates() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id = update_import(&pool, &ws).await;
    let before_v1 = Restorable {
        location: Some("Field kit 2".to_string()),
        notes: Some("Dented".to_string()),
        status: "in_repair".to_string(),
        name: "V-1".to_string(),
    };
    assert_eq!(restorable(&pool, "V-1"), before_v1);

    let (status, file) = previous_values(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{file}");
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        crate::imports_assets::upload_request("previous-values.csv", file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let restore = id_of(&job);
    let mapping: serde_json::Map<String, serde_json::Value> = job["suggested_mapping"]
        .as_array()
        .expect("suggested mapping")
        .iter()
        .map(|s| {
            (
                s["field"].as_str().expect("field").to_string(),
                s["index"].clone(),
            )
        })
        .collect();
    assert_eq!(
        serde_json::Value::Object(mapping.clone()),
        serde_json::json!({ "asset_tag": 0, "status": 1, "location": 2, "notes": 3 }),
        "every header maps to its field, with no Name or Kind column"
    );
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        crate::imports_assets::mapping_request(
            &restore,
            serde_json::json!({ "mapping": mapping, "options": {} }),
        ),
    )
    .await;
    let checked = json(&body);
    assert_eq!(status, StatusCode::OK, "{checked}");
    assert_eq!(checked["summary"]["would_update"], 2, "{checked}");
    assert_eq!(checked["summary"]["would_create"], 0, "{checked}");
    assert_eq!(checked["summary"]["rows_with_errors"], 0, "{checked}");
    commit(&pool, &ws, &restore).await;

    assert_eq!(
        restorable(&pool, "V-1"),
        Restorable {
            location: Some("Studio A".to_string()),
            notes: Some("Dented".to_string()),
            status: "in_service".to_string(),
            name: "V-1".to_string(),
        }
    );
    assert_eq!(
        restorable(&pool, "V-2").location.as_deref(),
        Some("Studio B")
    );
    let rows = crate::imports_attributes::rows(&pool, &ws, &restore).await;
    assert!(rows.iter().all(|r| r["outcome"] == "update"), "{rows:?}");
}
