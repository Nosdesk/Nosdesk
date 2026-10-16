//! A file with only a key and the columns to change (design §7, Sam's
//! repeat import): Name and Kind are needed to create an asset, never to
//! update one. Under Create and update the matched rows update and a row
//! that would create without them is reported on Review; under Only update
//! existing nothing asks for them; under Only create new Map refuses the
//! mapping, since every row creates.

use actix_web::http::StatusCode;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text};

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{
    call_import_route, import_workspace, json, mapping_request, upload_request,
};
use crate::imports_attributes::{checked, commit, errors, id_of, rows};

const MOVES: &str = "asset_tag,location\n\
    QA-I1-001,QA i1 Room 4\n\
    QA-I1-002,QA i1 Room 4\n\
    QA-I1-003,QA i1 Room 4\n\
    QA-I1-004,QA i1 Room 4\n\
    QA-I1-009,QA i1 Room 4\n\
    QA-I1-010,Warehouse\n";

const MAPPING: &[(&str, &str)] = &[("asset_tag", "asset_tag"), ("location", "location")];

/// 001 to 004 in Studio A, 010 already in the Warehouse; no 009.
fn seed_assets(pool: &TestPool, ws: &WorkspaceSeed) {
    for (tag, location) in [
        ("QA-I1-001", "Studio A"),
        ("QA-I1-002", "Studio A"),
        ("QA-I1-003", "Studio A"),
        ("QA-I1-004", "Studio A"),
        ("QA-I1-010", "Warehouse"),
    ] {
        diesel::sql_query(
            "INSERT INTO assets (name, kind, asset_tag, location, status, workspace_id)
             VALUES ($1, 'generic', $2, $3, 'in_service', $4)",
        )
        .bind::<Text, _>(format!("Camera {tag}"))
        .bind::<Text, _>(tag)
        .bind::<Text, _>(location)
        .bind::<Integer, _>(ws.workspace_id)
        .execute(&mut pool.get().expect("conn"))
        .expect("seed asset");
    }
}

#[derive(QueryableByName, Debug, PartialEq)]
struct Stored {
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = Nullable<Text>)]
    location: Option<String>,
}

fn stored(pool: &TestPool, tag: &str) -> Option<Stored> {
    diesel::sql_query("SELECT name, kind, location FROM assets WHERE asset_tag = $1")
        .bind::<Text, _>(tag)
        .get_result(&mut pool.get().expect("conn"))
        .optional()
        .expect("read asset")
}

fn outcome_of(rows: &[serde_json::Value], tag: &str) -> String {
    rows.iter()
        .find(|r| r["key"] == tag)
        .unwrap_or_else(|| panic!("no row for {tag}: {rows:?}"))["outcome"]
        .as_str()
        .expect("outcome")
        .to_string()
}

/// The four moved assets are in Room 4 with their names and kinds, and
/// 010 is where it was.
fn assert_moved(pool: &TestPool) {
    for tag in ["QA-I1-001", "QA-I1-002", "QA-I1-003", "QA-I1-004"] {
        assert_eq!(
            stored(pool, tag),
            Some(Stored {
                name: format!("Camera {tag}"),
                kind: "generic".to_string(),
                location: Some("QA i1 Room 4".to_string()),
            }),
            "{tag}"
        );
    }
    assert_eq!(
        stored(pool, "QA-I1-010").and_then(|s| s.location),
        Some("Warehouse".to_string())
    );
}

/// Create and update: Map takes the file, the matched rows update or read
/// Unchanged, and 009, which would create an asset with no name and no
/// kind, is a row error fixed on Map columns.
#[actix_web::test]
async fn create_and_update_updates_matched_rows_and_reports_the_new_one() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_assets(&pool, &ws);

    let job = checked(&pool, &ws, MOVES, MAPPING, serde_json::json!({})).await;
    let summary = &job["summary"];
    assert_eq!(summary["would_update"], 4, "{summary}");
    assert_eq!(summary["unchanged"], 1, "{summary}");
    assert_eq!(summary["would_create"], 0, "{summary}");
    assert_eq!(summary["rows_with_errors"], 1, "{summary}");
    assert_eq!(
        errors(&job),
        vec![
            (6, Some("name".to_string()), "name is required".to_string()),
            (6, Some("kind".to_string()), "kind is required".to_string()),
        ]
    );
    let groups = summary["error_groups"].as_array().expect("groups");
    assert!(
        groups.iter().all(|g| g["fix"] == "map"),
        "a name or kind nothing gives is fixed on Map columns: {groups:?}"
    );
    let changes: Vec<&str> = summary["changes"]
        .as_array()
        .expect("changes")
        .iter()
        .map(|c| c["field"].as_str().expect("field"))
        .collect();
    assert_eq!(changes, vec!["location"], "{summary}");

    let id = id_of(&job);
    let review = rows(&pool, &ws, &id).await;
    let updated = review
        .iter()
        .find(|r| r["key"] == "QA-I1-001")
        .expect("001");
    assert_eq!(
        updated["label"], "Camera QA-I1-001",
        "a matched row with no name reads as the asset's name"
    );

    commit(&pool, &ws, &id).await;
    let after = rows(&pool, &ws, &id).await;
    for tag in ["QA-I1-001", "QA-I1-002", "QA-I1-003", "QA-I1-004"] {
        assert_eq!(outcome_of(&after, tag), "update", "{tag}");
    }
    assert_eq!(outcome_of(&after, "QA-I1-010"), "unchanged");
    assert_eq!(outcome_of(&after, "QA-I1-009"), "skipped");
    assert_moved(&pool);
    assert_eq!(stored(&pool, "QA-I1-009"), None);
}

/// Create and update with a kind and a name for every new row: 009 is
/// created with them, and the matched rows keep their own names and kinds.
#[actix_web::test]
async fn values_for_new_rows_create_the_missing_asset_and_leave_the_rest() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_assets(&pool, &ws);

    let options = serde_json::json!({
        "constants": [
            { "field": "kind", "value": "generic", "apply_to": "new" },
            { "field": "name", "value": "Room 4 spare", "apply_to": "new" }
        ]
    });
    let job = checked(&pool, &ws, MOVES, MAPPING, options).await;
    let summary = &job["summary"];
    assert_eq!(summary["would_update"], 4, "{summary}");
    assert_eq!(summary["unchanged"], 1, "{summary}");
    assert_eq!(summary["would_create"], 1, "{summary}");
    assert_eq!(summary["rows_with_errors"], 0, "{summary}");

    commit(&pool, &ws, &id_of(&job)).await;
    assert_moved(&pool);
    assert_eq!(
        stored(&pool, "QA-I1-009"),
        Some(Stored {
            name: "Room 4 spare".to_string(),
            kind: "generic".to_string(),
            location: Some("QA i1 Room 4".to_string()),
        })
    );
}

/// Only update existing: no Name or Kind asked for, the matched rows
/// update, and 009 is skipped as not found.
#[actix_web::test]
async fn only_update_existing_needs_no_name_or_kind() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_assets(&pool, &ws);

    let job = checked(
        &pool,
        &ws,
        MOVES,
        MAPPING,
        serde_json::json!({ "mode": "update_only" }),
    )
    .await;
    let summary = &job["summary"];
    assert_eq!(summary["would_update"], 4, "{summary}");
    assert_eq!(summary["unchanged"], 1, "{summary}");
    assert_eq!(summary["would_create"], 0, "{summary}");
    assert_eq!(
        errors(&job),
        vec![(
            6,
            Some("asset_tag".to_string()),
            "not found: no asset has this tag".to_string()
        )]
    );

    let id = id_of(&job);
    commit(&pool, &ws, &id).await;
    let after = rows(&pool, &ws, &id).await;
    assert_eq!(outcome_of(&after, "QA-I1-009"), "skipped");
    assert_moved(&pool);
    assert_eq!(stored(&pool, "QA-I1-009"), None);
}

/// Only create new: every row creates, so Map refuses a file with no Name.
#[actix_web::test]
async fn only_create_new_still_needs_a_name() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_assets(&pool, &ws);

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        upload_request("moves.csv", MOVES.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(
            &id_of(&job),
            serde_json::json!({
                "mapping": { "asset_tag": 0, "location": 1 },
                "options": {
                    "mode": "create_only",
                    "constants": [{ "field": "kind", "value": "generic", "apply_to": "new" }]
                }
            }),
        ),
    )
    .await;
    let resp = json(&body);
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert!(
        resp["error"]
            .as_str()
            .unwrap_or("")
            .contains("no column maps to Name"),
        "{resp}"
    );
}
