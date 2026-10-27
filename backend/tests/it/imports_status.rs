//! The Status column: values matched to the lifecycle statuses (labels,
//! saved answers, known other names), disposed and on loan listed but not
//! pickable, a new asset's Imported event followed by its status move,
//! updates moved through the lifecycle rules, the dry run refusing what
//! those rules refuse, an unchanged status writing nothing, and the commit
//! refusing a plan whose status moves changed since the check.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Jsonb, Nullable, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::models::NewImportValueAnswer;
use backend::repository::imports as repo;
use backend::sync::session::run_in_workspace;

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{call_import_route, import_workspace, json};
use crate::imports_attributes::{checked, commit, errors, id_of, rows};

const REF: &str = "test:imports_status";

const MAPPING: &[(&str, &str)] = &[("Name", "name"), ("Tag", "asset_tag"), ("Status", "status")];

fn options() -> serde_json::Value {
    serde_json::json!({ "default_kind": "generic" })
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// An asset named and tagged `tag`, in `status`, optionally synced.
fn seed_asset(pool: &TestPool, ws: &WorkspaceSeed, tag: &str, status: &str, synced: bool) -> i32 {
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, status, external_sync_source, workspace_id)
         VALUES ($1, 'generic', $1, $2, $3, $4) RETURNING id",
    )
    .bind::<Text, _>(tag)
    .bind::<Text, _>(status)
    .bind::<Nullable<Text>, _>(synced.then_some("intune"))
    .bind::<Integer, _>(ws.workspace_id)
    .get_result::<IdRow>(&mut pool.get().expect("conn"))
    .expect("seed asset")
    .id
}

/// An open loan on `asset`, as issuing one leaves it.
fn seed_loan(pool: &TestPool, ws: &WorkspaceSeed, asset: i32) {
    diesel::sql_query(
        "INSERT INTO asset_loans (asset_id, borrower_user_uuid, status_before, workspace_id)
         VALUES ($1, $2, 'in_service', $3)",
    )
    .bind::<Integer, _>(asset)
    .bind::<SqlUuid, _>(ws.member_uuid)
    .bind::<Integer, _>(ws.workspace_id)
    .execute(&mut pool.get().expect("conn"))
    .expect("seed loan");
}

fn set_status(pool: &TestPool, asset: i32, status: &str) {
    diesel::sql_query("UPDATE assets SET status = $1 WHERE id = $2")
        .bind::<Text, _>(status)
        .bind::<Integer, _>(asset)
        .execute(&mut pool.get().expect("conn"))
        .expect("set status");
}

#[derive(QueryableByName)]
struct AssetRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Text)]
    status: String,
    #[diesel(sql_type = Text)]
    updated_at: String,
}

fn asset(pool: &TestPool, tag: &str) -> AssetRow {
    diesel::sql_query(
        "SELECT id, status, to_char(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US') AS updated_at
         FROM assets WHERE asset_tag = $1",
    )
    .bind::<Text, _>(tag)
    .get_result(&mut pool.get().expect("conn"))
    .expect("read asset")
}

#[derive(QueryableByName, Debug)]
struct EventRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Nullable<Text>)]
    from_status: Option<String>,
    #[diesel(sql_type = Text)]
    to_status: String,
    #[diesel(sql_type = Nullable<Text>)]
    reason: Option<String>,
    #[diesel(sql_type = Jsonb)]
    metadata: serde_json::Value,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    actor_uuid: Option<Uuid>,
}

fn events(pool: &TestPool, asset: i32) -> Vec<EventRow> {
    diesel::sql_query(
        "SELECT id, from_status, to_status, reason, metadata, actor_uuid
         FROM asset_lifecycle_events WHERE asset_id = $1 ORDER BY id",
    )
    .bind::<Integer, _>(asset)
    .load(&mut pool.get().expect("conn"))
    .expect("read events")
}

/// Sync actions recorded for the asset's lifecycle events.
fn event_sync_actions(pool: &TestPool, asset: i32) -> usize {
    events(pool, asset)
        .iter()
        .map(|e| {
            #[derive(QueryableByName)]
            struct Count {
                #[diesel(sql_type = diesel::sql_types::BigInt)]
                count: i64,
            }
            diesel::sql_query(
                "SELECT COUNT(*) AS count FROM sync_actions
                 WHERE aggregate::text = 'asset_lifecycle_event' AND aggregate_id = $1",
            )
            .bind::<Text, _>(e.id.to_string())
            .get_result::<Count>(&mut pool.get().expect("conn"))
            .expect("count")
            .count as usize
        })
        .sum()
}

fn file(rows: &[(&str, &str)]) -> String {
    let mut out = String::from("Name,Tag,Status\n");
    for (tag, status) in rows {
        out.push_str(&format!("{tag},{tag},{status}\n"));
    }
    out
}

async fn get_values(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> serde_json::Value {
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

async fn put_values(
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

fn status_column(view: &serde_json::Value) -> &serde_json::Value {
    view["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|c| c["field"] == "status")
        .unwrap_or_else(|| panic!("no status column in {view}"))
}

fn value<'a>(column: &'a serde_json::Value, text: &str) -> &'a serde_json::Value {
    column["values"]
        .as_array()
        .expect("values")
        .iter()
        .find(|v| v["text"] == text)
        .unwrap_or_else(|| panic!("no value '{text}' in {column}"))
}

fn link(target: &str) -> serde_json::Value {
    serde_json::json!({ "action": "match", "target": target })
}

/// The row result for file line `row`.
fn row_result(rows: &[serde_json::Value], row: u64) -> &serde_json::Value {
    rows.iter()
        .find(|r| r["row"] == row)
        .unwrap_or_else(|| panic!("no row {row} in {rows:?}"))
}

/// Status values match a status's key or label exactly, then a saved
/// answer, then a known other name (Guessed). Disposed and on loan are
/// listed with the note and can't be picked: a value naming one, or a
/// known other name for one, has no match and leaves the status as it
/// is, and `PUT values` refuses an answer linking to one.
#[actix_web::test]
async fn status_values_match_in_layers_and_disposed_and_on_loan_cant_be_picked() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    run_in_workspace(&db.runtime_pool(2), REF, ws.workspace_id, |c| {
        repo::upsert_value_answers(
            c,
            &[NewImportValueAnswer {
                job_type: "assets".to_string(),
                field: "status".to_string(),
                context: String::new(),
                normalized_value: "u/s".to_string(),
                answer: link("in_repair"),
                answered_by: Some(ws.admin_uuid),
                job_id: None,
                source_text: None,
                created: false,
            }],
        )
    })
    .expect("save an answer");

    let job = checked(
        &pool,
        &ws,
        &file(&[
            ("S-1", "In service"),
            ("S-2", "in_repair"),
            ("S-3", "In use"),
            ("S-4", "Spare"),
            ("S-5", "Broken"),
            ("S-6", "U/S"),
            ("S-7", "Loaned"),
            ("S-8", "Disposed"),
            ("S-9", "Somewhere"),
            ("S-10", ""),
        ]),
        MAPPING,
        options(),
    )
    .await;
    assert_eq!(errors(&job), vec![], "{job}");
    assert_eq!(job["summary"]["would_create"], 10, "{job}");
    let id = id_of(&job);

    let view = get_values(&pool, &ws, &id).await;
    let status = status_column(&view);
    let choices = status["choices"].as_array().expect("choices");
    let keys: Vec<&str> = choices
        .iter()
        .map(|c| c["value"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        vec![
            "in_service",
            "in_stock",
            "on_order",
            "in_transit",
            "in_repair",
            "on_loan",
            "retired",
            "lost",
            "disposed"
        ]
    );
    let disabled: Vec<(&str, &str)> = choices
        .iter()
        .filter_map(|c| Some((c["value"].as_str()?, c["disabled"].as_str()?)))
        .collect();
    assert_eq!(
        disabled,
        vec![
            ("on_loan", "Set by a disposal or a loan after import"),
            ("disposed", "Set by a disposal or a loan after import"),
        ]
    );

    let read = |text: &str| {
        let v = value(status, text);
        (v["match"].clone(), v["suggested"].clone())
    };
    assert_eq!(
        read("In service"),
        (serde_json::json!("exact"), link("in_service"))
    );
    assert_eq!(
        read("in_repair"),
        (serde_json::json!("exact"), link("in_repair"))
    );
    assert_eq!(read("U/S"), (serde_json::json!("saved"), link("in_repair")));
    for (text, target) in [
        ("In use", "in_service"),
        ("Spare", "in_stock"),
        ("Broken", "in_repair"),
    ] {
        assert_eq!(read(text), (serde_json::json!("guessed"), link(target)));
        assert_eq!(
            value(status, text)["reason"],
            serde_json::json!({ "code": "synonym" })
        );
    }
    let leave = serde_json::json!({ "action": "leave_blank" });
    for (text, label) in [("Loaned", "On loan"), ("Disposed", "Disposed")] {
        assert_eq!(read(text), (serde_json::json!("no_match"), leave.clone()));
        assert_eq!(
            value(status, text)["reason"],
            serde_json::json!({
                "code": "disabled",
                "label": label,
                "note": "Set by a disposal or a loan after import"
            })
        );
    }
    assert_eq!(
        read("Somewhere"),
        (serde_json::json!("no_match"), leave.clone())
    );

    for target in ["on_loan", "disposed"] {
        let (code, body) = put_values(
            &pool,
            &ws,
            &id,
            serde_json::json!({ "status": { "": { "Loaned": link(target) } } }),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or("")
                .contains("isn't one of the choices"),
            "{body}"
        );
    }

    // What the commit makes of them: matched values move, the rest start
    // in service.
    commit(&pool, &ws, &id).await;
    for (tag, expected) in [
        ("S-1", "in_service"),
        ("S-2", "in_repair"),
        ("S-3", "in_service"),
        ("S-4", "in_stock"),
        ("S-5", "in_repair"),
        ("S-6", "in_repair"),
        ("S-7", "in_service"),
        ("S-8", "in_service"),
        ("S-9", "in_service"),
        ("S-10", "in_service"),
    ] {
        assert_eq!(asset(&pool, tag).status, expected, "{tag}");
    }
}

/// A new asset gets its Imported event, then a move to the row's status
/// through the lifecycle rules (reason Imported, naming the job, by the
/// admin), and the row keeps that move as its `transition_id`, the status
/// among what it wrote, and the asset's `updated_at` as it stands after the
/// move. A blank status, or In service, leaves the asset in service with
/// the Imported event alone.
#[actix_web::test]
async fn a_new_asset_moves_to_its_status_after_the_imported_event() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let job = checked(
        &pool,
        &ws,
        &file(&[("N-1", "In repair"), ("N-2", ""), ("N-3", "In service")]),
        MAPPING,
        options(),
    )
    .await;
    assert_eq!(errors(&job), vec![], "{job}");
    let id = id_of(&job);
    let checked_rows = rows(&pool, &ws, &id).await;
    assert_eq!(
        row_result(&checked_rows, 2)["fields_written"]["status"],
        serde_json::json!({ "before": null, "after": "in_repair" })
    );
    assert!(row_result(&checked_rows, 3).get("fields_written").is_none());

    commit(&pool, &ws, &id).await;
    let done = rows(&pool, &ws, &id).await;

    let moved = asset(&pool, "N-1");
    assert_eq!(moved.status, "in_repair");
    let history = events(&pool, moved.id);
    assert_eq!(history.len(), 2, "{history:?}");
    let (imported, transition) = (&history[0], &history[1]);
    assert_eq!(imported.from_status, None);
    assert_eq!(imported.to_status, "in_service");
    assert_eq!(imported.reason.as_deref(), Some("Imported"));
    assert_eq!(transition.from_status.as_deref(), Some("in_service"));
    assert_eq!(transition.to_status, "in_repair");
    assert_eq!(transition.reason.as_deref(), Some("Imported"));
    for event in &history {
        assert_eq!(event.metadata, serde_json::json!({ "import_job_id": id }));
        assert_eq!(event.actor_uuid, Some(ws.admin_uuid));
    }
    assert_eq!(event_sync_actions(&pool, moved.id), 2);

    let n1 = row_result(&done, 2);
    assert_eq!(n1["outcome"], "create");
    assert_eq!(n1["transition_id"], transition.id);
    assert_eq!(
        n1["fields_written"]["status"],
        serde_json::json!({ "before": null, "after": "in_repair" })
    );
    assert_eq!(n1["observed_updated_at"], moved.updated_at);

    for (row, tag) in [(3, "N-2"), (4, "N-3")] {
        let stayed = asset(&pool, tag);
        assert_eq!(stayed.status, "in_service", "{tag}");
        let history = events(&pool, stayed.id);
        assert_eq!(history.len(), 1, "{tag}: {history:?}");
        let result = row_result(&done, row);
        assert_eq!(result["transition_id"], history[0].id, "{tag}");
        assert_eq!(
            result["fields_written"]["status"],
            serde_json::json!({ "before": null, "after": "in_service" }),
            "{tag}"
        );
    }
}

/// An existing asset moves through the lifecycle rules and the row keeps
/// the status before and after. A status the asset already has writes no
/// event and isn't among what the row wrote; a synced asset whose row
/// matches it, status included, stays Unchanged, and one whose row moves
/// only its status is updated.
#[actix_web::test]
async fn an_update_moves_the_status_and_an_unchanged_status_writes_nothing() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let u1 = seed_asset(&pool, &ws, "U-1", "in_service", false);
    let u2 = seed_asset(&pool, &ws, "U-2", "in_repair", false);
    let s1 = seed_asset(&pool, &ws, "S-1", "in_stock", true);
    let s2 = seed_asset(&pool, &ws, "S-2", "in_service", true);
    let s1_before = asset(&pool, "S-1").updated_at;

    let job = checked(
        &pool,
        &ws,
        &file(&[
            ("U-1", "Spare"),
            ("U-2", "In repair"),
            ("S-1", "In stock"),
            ("S-2", "Broken"),
        ]),
        MAPPING,
        options(),
    )
    .await;
    assert_eq!(errors(&job), vec![], "{job}");
    assert_eq!(job["summary"]["would_update"], 2, "{job}");
    assert_eq!(job["summary"]["unchanged"], 2, "{job}");
    let id = id_of(&job);
    commit(&pool, &ws, &id).await;
    let done = rows(&pool, &ws, &id).await;

    assert_eq!(asset(&pool, "U-1").status, "in_stock");
    let history = events(&pool, u1);
    assert_eq!(history.len(), 1, "{history:?}");
    assert_eq!(history[0].from_status.as_deref(), Some("in_service"));
    assert_eq!(history[0].to_status, "in_stock");
    assert_eq!(history[0].reason.as_deref(), Some("Imported"));
    assert_eq!(
        history[0].metadata,
        serde_json::json!({ "import_job_id": id })
    );
    assert_eq!(history[0].actor_uuid, Some(ws.admin_uuid));
    let r1 = row_result(&done, 2);
    assert_eq!(r1["outcome"], "update");
    assert_eq!(r1["transition_id"], history[0].id);
    assert_eq!(
        r1["fields_written"]["status"],
        serde_json::json!({ "before": "in_service", "after": "in_stock" })
    );
    assert_eq!(r1["observed_updated_at"], asset(&pool, "U-1").updated_at);

    assert_eq!(asset(&pool, "U-2").status, "in_repair");
    assert!(events(&pool, u2).is_empty());
    let r2 = row_result(&done, 3);
    assert_eq!(r2["outcome"], "unchanged");
    assert!(r2.get("fields_written").is_none(), "{r2}");
    assert!(r2.get("transition_id").is_none(), "{r2}");

    let r3 = row_result(&done, 4);
    assert_eq!(r3["outcome"], "unchanged");
    assert!(events(&pool, s1).is_empty());
    assert_eq!(asset(&pool, "S-1").updated_at, s1_before);

    assert_eq!(asset(&pool, "S-2").status, "in_repair");
    let r4 = row_result(&done, 5);
    assert_eq!(r4["outcome"], "update");
    assert_eq!(events(&pool, s2).len(), 1);
    assert_eq!(
        r4["fields_written"]["status"],
        serde_json::json!({ "before": "in_service", "after": "in_repair" })
    );
}

/// The dry run refuses what the lifecycle rules would, each a row error on
/// the Status column in the admin's words: a disposed asset keeps its
/// status, one out on loan leaves it only by its return, and on loan and
/// disposed come from a loan and a disposal. An asset left on loan with no
/// loan out can be moved, and a status an asset already has is no move.
#[actix_web::test]
async fn the_dry_run_refuses_what_the_lifecycle_rules_refuse() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_asset(&pool, &ws, "D-1", "disposed", false);
    seed_asset(&pool, &ws, "D-2", "disposed", false);
    let loaned = seed_asset(&pool, &ws, "L-1", "on_loan", false);
    seed_loan(&pool, &ws, loaned);
    seed_asset(&pool, &ws, "X-1", "on_loan", false);

    let job = checked(
        &pool,
        &ws,
        &file(&[
            ("D-1", "In service"),
            ("D-2", "Disposed"),
            ("L-1", "In stock"),
            ("X-1", "In stock"),
        ]),
        MAPPING,
        options(),
    )
    .await;
    assert_eq!(
        errors(&job),
        vec![
            (
                2,
                Some("status".to_string()),
                "D-1 is disposed, and disposal is final; its status can't change".to_string()
            ),
            (
                4,
                Some("status".to_string()),
                "L-1 is out on loan; return the loan on the asset page to change its status"
                    .to_string()
            ),
        ],
        "{job}"
    );
    assert_eq!(job["summary"]["errors"][0]["source_column"], "Status");
    // X-1 moves; D-2 already has its status and changes nothing.
    assert_eq!(job["summary"]["would_update"], 1, "{job}");
    assert_eq!(job["summary"]["unchanged"], 1, "{job}");

    commit(&pool, &ws, &id_of(&job)).await;
    assert_eq!(asset(&pool, "D-1").status, "disposed");
    assert_eq!(asset(&pool, "D-2").status, "disposed");
    assert_eq!(asset(&pool, "L-1").status, "on_loan");
    assert_eq!(asset(&pool, "X-1").status, "in_stock");

    // On loan and disposed set for every row: refused on an existing asset
    // and on a new one.
    seed_asset(&pool, &ws, "N-1", "in_service", false);
    for (value, apply_to, tags, message) in [
        (
            "On loan",
            "all",
            "N-1",
            "'On loan' is set by issuing a loan on the asset page",
        ),
        (
            "disposed",
            "new",
            "NEW-1",
            "'disposed' is set by recording a disposal on the asset page",
        ),
    ] {
        let job = checked(
            &pool,
            &ws,
            &format!("Name,Tag\n{tags},{tags}\n"),
            &[("Name", "name"), ("Tag", "asset_tag")],
            serde_json::json!({
                "default_kind": "generic",
                "constants": [{ "field": "status", "value": value, "apply_to": apply_to }],
            }),
        )
        .await;
        assert_eq!(
            errors(&job),
            vec![(2, Some("status".to_string()), message.to_string())],
            "{job}"
        );
    }
}

/// A status moved between the check and the commit changes the plan: the
/// commit writes nothing and answers 409 with the fresh plan, which then
/// commits.
#[actix_web::test]
async fn a_status_moved_since_the_check_is_refused_with_the_fresh_plan() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let id_asset = seed_asset(&pool, &ws, "M-1", "in_service", false);
    let job = checked(
        &pool,
        &ws,
        &file(&[("M-1", "In repair")]),
        MAPPING,
        options(),
    )
    .await;
    assert_eq!(errors(&job), vec![], "{job}");
    let id = id_of(&job);

    set_status(&pool, id_asset, "in_stock");
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
    assert_eq!(asset(&pool, "M-1").status, "in_stock");
    assert!(events(&pool, id_asset).is_empty());
    let fresh = rows(&pool, &ws, &id).await;
    assert_eq!(
        row_result(&fresh, 2)["fields_written"]["status"],
        serde_json::json!({ "before": "in_stock", "after": "in_repair" })
    );

    commit(&pool, &ws, &id).await;
    assert_eq!(asset(&pool, "M-1").status, "in_repair");
    let history = events(&pool, id_asset);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].from_status.as_deref(), Some("in_stock"));
}
