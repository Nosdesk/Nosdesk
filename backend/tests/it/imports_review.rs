//! What Review reads: a matched row that changes nothing is Unchanged and
//! the commit writes nothing for it; the summary's per-field change counts,
//! per-kind new counts and columns no field takes; where each problem is
//! fixed; the error report with its skipped and partly imported rows,
//! uploaded again once fixed; and the report narrowed to one problem's rows
//! or to the rows that import without some of their values.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Text};

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{call_import_route, import_workspace, import_workspaces};
use crate::imports_attributes::{checked, commit, id_of, rows, seed_kind};

fn vehicle_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "fuel": { "type": "string", "title": "Fuel", "enum": ["Diesel", "Petrol"] },
            "rego": { "type": "string", "title": "Rego" }
        },
        "required": ["fuel"]
    })
}

fn camera_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "mount": { "type": "string", "title": "Mount" }
        }
    })
}

fn seed_kinds(pool: &TestPool, ws: &WorkspaceSeed) {
    seed_kind(pool, ws, "vehicle", "Vehicle", 10, vehicle_schema());
    seed_kind(pool, ws, "camera", "Camera", 11, camera_schema());
}

#[derive(diesel::QueryableByName)]
struct IdRow {
    #[diesel(sql_type = Integer)]
    id: i32,
}

fn seed_asset(pool: &TestPool, ws: &WorkspaceSeed, tag: &str, kind: &str, location: &str) -> i32 {
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, location, status, workspace_id)
         VALUES ($1, $2, $1, $3, 'in_service', $4) RETURNING id",
    )
    .bind::<Text, _>(tag)
    .bind::<Text, _>(kind)
    .bind::<Text, _>(location)
    .bind::<Integer, _>(ws.workspace_id)
    .get_result::<IdRow>(&mut pool.get().expect("conn"))
    .expect("seed asset")
    .id
}

fn updated_at(pool: &TestPool, id: i32) -> chrono::NaiveDateTime {
    use backend::schema::assets;
    assets::table
        .find(id)
        .select(assets::updated_at)
        .first(&mut pool.get().expect("conn"))
        .expect("read asset")
}

/// Sync actions recorded for one asset.
fn sync_actions_for(pool: &TestPool, id: i32) -> i64 {
    use backend::models::SyncAggregate;
    use backend::schema::sync_actions;
    sync_actions::table
        .filter(sync_actions::aggregate.eq(SyncAggregate::Asset))
        .filter(sync_actions::aggregate_id.eq(id.to_string()))
        .count()
        .get_result(&mut pool.get().expect("conn"))
        .expect("count sync actions")
}

const MAPPING: &[(&str, &str)] = &[
    ("Name", "name"),
    ("Tag", "asset_tag"),
    ("Kind", "kind"),
    ("Location", "location"),
    ("Status", "status"),
    ("Fuel", "fuel"),
    ("Rego", "rego"),
    ("Assigned To", "primary_user"),
];

const HEADER: &str = "Name,Tag,Kind,Location,Status,Fuel,Rego,Assigned To,Room";

fn row_of(rows: &[serde_json::Value], row: u64) -> &serde_json::Value {
    rows.iter()
        .find(|r| r["row"] == row)
        .unwrap_or_else(|| panic!("no row {row}"))
}

/// A row that matches an asset and holds what it holds is Unchanged: the
/// commit writes nothing for it, so no sync action and no change recorded.
/// An updated row carries each change before and after, which the summary
/// counts per field; new rows are counted per kind; a column no field takes
/// is counted with its values.
#[actix_web::test]
async fn an_identical_row_is_unchanged_and_writes_nothing() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_kinds(&pool, &ws);
    let same = seed_asset(&pool, &ws, "A-1", "camera", "Studio A");
    let moved = seed_asset(&pool, &ws, "A-2", "camera", "Studio A");
    diesel::sql_query("INSERT INTO asset_groups (name, workspace_id) VALUES ('Field kit', $1)")
        .bind::<Integer, _>(ws.workspace_id)
        .execute(&mut pool.get().expect("conn"))
        .expect("seed group");
    let same_before = updated_at(&pool, same);
    let same_actions = sync_actions_for(&pool, same);

    let job = checked(
        &pool,
        &ws,
        &format!(
            "{HEADER},Groups\n\
             A-1,A-1,Camera,Studio A,In service,,,,R1,\n\
             A-2,A-2,Camera,Studio B,In repair,,,,,Field kit\n\
             N-1,N-1,Vehicle,Yard,,Diesel,ABC123,,R2,\n\
             N-2,N-2,Camera,Studio A,,,XYZ9,nobody@example.com,,\n\
             N-3,N-3,Camera,Studio B,,,,,R3,\n"
        ),
        &[MAPPING, &[("Groups", "groups")]].concat(),
        serde_json::json!({}),
    )
    .await;
    let summary = &job["summary"];
    assert_eq!(summary["errors_total"], 0, "{job}");
    assert_eq!(
        (
            &summary["would_create"],
            &summary["would_update"],
            &summary["unchanged"]
        ),
        (
            &serde_json::json!(3),
            &serde_json::json!(1),
            &serde_json::json!(1)
        ),
        "{summary}"
    );
    assert_eq!(
        summary["changes"],
        serde_json::json!([
            { "field": "groups", "source_column": "Groups", "count": 1, "constant": false },
            { "field": "location", "source_column": "Location", "count": 1, "constant": false },
            { "field": "status", "source_column": "Status", "count": 1, "constant": false },
        ]),
        "{summary}"
    );
    assert_eq!(
        summary["kinds"],
        serde_json::json!([
            { "kind": "camera", "count": 2 },
            { "kind": "vehicle", "count": 1 },
        ]),
        "{summary}"
    );
    // Room has no field: three rows hold a value in it.
    assert_eq!(
        summary["unmapped"],
        serde_json::json!([{ "index": 8, "values": 3 }]),
        "{summary}"
    );

    let id = id_of(&job);
    let checked_rows = rows(&pool, &ws, &id).await;
    let unchanged = row_of(&checked_rows, 2);
    assert_eq!(unchanged["outcome"], "unchanged", "{unchanged}");
    assert_eq!(unchanged["kind"], "camera");
    assert!(unchanged.get("fields_written").is_none(), "{unchanged}");
    let update = row_of(&checked_rows, 3);
    assert_eq!(update["outcome"], "update", "{update}");
    assert_eq!(
        update["fields_written"],
        serde_json::json!({
            "groups": { "before": null, "after": "Field kit" },
            "location": { "before": "Studio A", "after": "Studio B" },
            "status": { "before": "in_service", "after": "in_repair" },
        })
    );

    commit(&pool, &ws, &id).await;
    let done = rows(&pool, &ws, &id).await;
    let unchanged = row_of(&done, 2);
    assert_eq!(unchanged["outcome"], "unchanged", "{unchanged}");
    assert!(unchanged.get("fields_written").is_none(), "{unchanged}");
    assert!(unchanged.get("written_at").is_none(), "{unchanged}");
    assert_eq!(updated_at(&pool, same), same_before);
    assert_eq!(sync_actions_for(&pool, same), same_actions);
    // The updated asset was written, and said so.
    assert!(sync_actions_for(&pool, moved) > 0);
    // The commit records what it wrote as the check said it would.
    assert_eq!(row_of(&done, 3)["fields_written"], update["fields_written"]);
}

/// Each problem says where it is fixed: a value Match values asks about
/// there, a kind attribute new assets need on Map columns, and a problem
/// only the file can fix (a tag twice in the file, a blank name) by
/// downloading its rows.
#[actix_web::test]
async fn each_problem_says_where_it_is_fixed() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_kinds(&pool, &ws);

    let job = checked(
        &pool,
        &ws,
        "Name,Tag,Kind\n\
         T-1,T-1,tripod\n\
         T-2,T-2,tripod\n\
         D-1,D-1,Camera\n\
         D-1 again,D-1,Camera\n\
         ,B-1,Camera\n\
         V-1,V-1,Vehicle\n",
        &[("Name", "name"), ("Tag", "asset_tag"), ("Kind", "kind")],
        serde_json::json!({}),
    )
    .await;
    let groups = job["summary"]["error_groups"]
        .as_array()
        .expect("groups")
        .clone();
    let fix_of = |column: &str, needle: &str| -> String {
        groups
            .iter()
            .find(|g| g["column"] == column && g["message"].as_str().unwrap().contains(needle))
            .unwrap_or_else(|| panic!("no {column} group with {needle}: {groups:?}"))["fix"]
            .as_str()
            .expect("fix")
            .to_string()
    };
    assert_eq!(fix_of("kind", "tripod"), "match_values");
    assert_eq!(fix_of("asset_tag", "appears twice"), "download");
    assert_eq!(fix_of("name", "required"), "download");
    assert_eq!(fix_of("fuel", "required for Vehicle"), "map");
}

async fn get(pool: &TestPool, ws: &WorkspaceSeed, uri: &str) -> (StatusCode, String) {
    let (status, body) =
        call_import_route(pool, ws, ws.admin_uuid, TestRequest::get().uri(uri)).await;
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// `?group=` downloads one problem's rows with their reasons, `?dropped=`
/// the rows that import without some of their values, each with why; both
/// in the file's layout. A non-admin is refused and another workspace's
/// job is not found.
#[actix_web::test]
async fn the_report_narrows_to_one_problem_or_to_left_out_values() {
    let db = TestDb::new();
    let admin_pool = db.pool_with_size(2);
    let seeded = import_workspaces(&admin_pool);
    seed_kinds(&admin_pool, &seeded.a);
    let pool = db.runtime_pool(4);
    let (a, b) = (&seeded.a, &seeded.b);

    let job = checked(
        &pool,
        a,
        &format!(
            "{HEADER}\n\
             D-1,D-1,Camera,,,,,,\n\
             D-1 again,D-1,Camera,,,,,,\n\
             ,B-1,Camera,,,,,,\n\
             N-2,N-2,Camera,Studio A,,,XYZ9,nobody@example.com,\n\
             N-3,N-3,Camera,Studio B,,,,,\n"
        ),
        MAPPING,
        serde_json::json!({}),
    )
    .await;
    let id = id_of(&job);
    let summary = &job["summary"];
    let group = summary["error_groups"]
        .as_array()
        .expect("groups")
        .iter()
        .position(|g| g["column"] == "asset_tag")
        .expect("duplicate tag group");
    let dropped = summary["dropped"].as_array().expect("dropped");
    let index_of = |column: &str| {
        dropped
            .iter()
            .position(|d| d["column"] == column)
            .unwrap_or_else(|| panic!("no {column} drop in {dropped:?}"))
    };
    let (rego, person) = (index_of("rego"), index_of("primary_user"));

    let (status, body) = get(
        &pool,
        a,
        &format!("/api/admin/import/{id}/error-report?group={group}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let lines: Vec<&str> = body.trim_start_matches('\u{FEFF}').lines().collect();
    assert_eq!(lines[0], format!("Row,Errors,{HEADER}"));
    assert_eq!(lines.len(), 3, "{body}");
    assert!(
        lines[1].starts_with("2,Tag: tag 'D-1' appears twice"),
        "{body}"
    );
    assert!(lines[2].starts_with("3,"), "{body}");

    let (status, body) = get(
        &pool,
        a,
        &format!("/api/admin/import/{id}/error-report?dropped={rego},{person}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let lines: Vec<&str> = body.trim_start_matches('\u{FEFF}').lines().collect();
    assert_eq!(
        lines[1..],
        [
            "5,\"Rego: Camera assets have no Rego, left out; Assigned To: 'nobody@example.com' is not a member of this workspace, left unassigned\",N-2,N-2,Camera,Studio A,,,XYZ9,nobody@example.com,",
        ],
        "{body}"
    );

    let (status, _) = get(
        &pool,
        a,
        &format!("/api/admin/import/{id}/error-report?group=99"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    for query in [format!("group={group}"), format!("dropped={rego}")] {
        let uri = format!("/api/admin/import/{id}/error-report?{query}");
        let (status, _) =
            call_import_route(&pool, a, a.member_uuid, TestRequest::get().uri(&uri)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{query}");
        assert_eq!(
            get(&pool, b, &uri).await.0,
            StatusCode::NOT_FOUND,
            "{query}"
        );
    }
}

/// Give the workspace's member a primary email the import can match.
fn seed_member_email(pool: &TestPool, ws: &WorkspaceSeed) -> String {
    let email = format!("sam-{}@example.com", ws.slug);
    diesel::sql_query(
        "INSERT INTO user_emails (user_uuid, email, is_primary, is_verified) VALUES ($1, $2, true, true)",
    )
    .bind::<diesel::sql_types::Uuid, _>(ws.member_uuid)
    .bind::<Text, _>(&email)
    .execute(&mut pool.get().expect("conn"))
    .expect("seed member email");
    email
}

fn report_lines(body: &str) -> Vec<String> {
    body.trim_start_matches('\u{FEFF}')
        .lines()
        .map(String::from)
        .collect()
}

/// The unfiltered report lists the skipped rows and the partly imported
/// ones together in file order: a partly imported row says it will be
/// imported (before the commit) or was (after), then which values it
/// leaves out. A row that imports whole is not listed.
#[actix_web::test]
async fn the_report_holds_skipped_and_partly_imported_rows() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_kinds(&pool, &ws);
    seed_asset(&pool, &ws, "U-1", "camera", "Studio A");

    let job = checked(
        &pool,
        &ws,
        &format!(
            "{HEADER}\n\
             D-1,D-1,Camera,,,,,,\n\
             D-1 again,D-1,Camera,,,,,,\n\
             ,B-1,Camera,,,,,,\n\
             N-2,N-2,Camera,Studio A,,,XYZ9,nobody@example.com,\n\
             N-3,N-3,Camera,Studio B,,,,,\n\
             U-1,U-1,Camera,Studio C,,,,nobody@example.com,\n"
        ),
        MAPPING,
        serde_json::json!({}),
    )
    .await;
    let id = id_of(&job);
    let uri = format!("/api/admin/import/{id}/error-report");
    let new_row = "Rego: Camera assets have no Rego, left out; Assigned To: 'nobody@example.com' is not a member of this workspace, left unassigned";
    let updated_row =
        "Assigned To: 'nobody@example.com' is not a member of this workspace, left unassigned";

    let (status, body) = get(&pool, &ws, &uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.starts_with('\u{FEFF}'), "BOM");
    let lines = report_lines(&body);
    assert_eq!(lines[0], format!("Row,Errors,{HEADER}"));
    let rows: Vec<&str> = lines[1..]
        .iter()
        .map(|l| l.split(',').next().unwrap())
        .collect();
    assert_eq!(rows, ["2", "3", "4", "5", "7"], "{body}");
    assert!(
        lines[1].starts_with("2,Tag: tag 'D-1' appears twice"),
        "{body}"
    );
    assert!(lines[3].starts_with("4,Name: name is required,"), "{body}");
    assert_eq!(
        lines[4],
        format!(
            "5,\"Will be imported without these values; {new_row}\",N-2,N-2,Camera,Studio A,,,XYZ9,nobody@example.com,"
        )
    );
    assert_eq!(
        lines[5],
        format!(
            "7,\"Will be imported without these values; {updated_row}\",U-1,U-1,Camera,Studio C,,,,nobody@example.com,"
        )
    );

    let done = commit(&pool, &ws, &id).await;
    assert_eq!(done["status"], "done", "{done}");
    let (status, body) = get(&pool, &ws, &uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let after = report_lines(&body);
    assert_eq!(after.len(), lines.len(), "{body}");
    assert_eq!(after[..4], lines[..4]);
    assert_eq!(
        after[4],
        format!(
            "5,\"Imported without these values; {new_row}\",N-2,N-2,Camera,Studio A,,,XYZ9,nobody@example.com,"
        )
    );
    assert!(
        after[5].starts_with(&format!(
            "7,\"Imported without these values; {updated_row}\""
        )),
        "{body}"
    );
}

/// The committed report, fixed and uploaded again: `Row` and `Errors` map
/// to nothing by rule, every other header comes back from the last
/// mapping, and matching by tag lands each fixed row. The rows that were
/// skipped are New; the partly imported ones, now naming a member, are
/// Updated with nothing left out.
#[actix_web::test]
async fn the_fixed_report_imports_skipped_rows_as_new_and_partly_imported_ones_as_updates() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_kinds(&pool, &ws);
    let member = seed_member_email(&pool, &ws);
    seed_asset(&pool, &ws, "U-1", "camera", "Studio A");

    let job = checked(
        &pool,
        &ws,
        &format!(
            "{HEADER}\n\
             D-1,D-1,Camera,,,,,,\n\
             D-1 again,D-1,Camera,,,,,,\n\
             ,B-1,Camera,,,,,,\n\
             N-2,N-2,Camera,Studio A,,,XYZ9,nobody@example.com,\n\
             N-3,N-3,Camera,Studio B,,,,,\n\
             U-1,U-1,Camera,Studio C,,,,nobody@example.com,\n"
        ),
        MAPPING,
        serde_json::json!({}),
    )
    .await;
    let id = id_of(&job);
    commit(&pool, &ws, &id).await;
    let (status, report) = get(&pool, &ws, &format!("/api/admin/import/{id}/error-report")).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    let fixed = report
        .replace("D-1 again,D-1,", "D-1 again,D-2,")
        .replace(",,B-1,", ",B-1,B-1,")
        .replace(",XYZ9,", ",,")
        .replace("nobody@example.com", &member);
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        crate::imports_assets::upload_request("assets-errors.csv", fixed.as_bytes()),
    )
    .await;
    let again = crate::imports_assets::json(&body);
    assert_eq!(status, StatusCode::OK, "{again}");
    let suggested = again["suggested_mapping"].as_array().expect("suggestions");
    let by_index = |i: usize| {
        suggested
            .iter()
            .find(|s| s["index"] == i)
            .unwrap_or_else(|| panic!("no suggestion for column {i}: {suggested:?}"))
    };
    for i in [0, 1] {
        assert_eq!(by_index(i)["reason"], "report", "{suggested:?}");
        assert!(by_index(i)["field"].is_null(), "{suggested:?}");
    }
    let mut mapping = serde_json::Map::new();
    for (header, field) in MAPPING {
        let index = HEADER.split(',').position(|h| h == *header).unwrap() + 2;
        assert_eq!(by_index(index)["field"], *field, "{suggested:?}");
        assert_eq!(by_index(index)["reason"], "previous", "{suggested:?}");
        mapping.insert(field.to_string(), index.into());
    }
    let again_id = again["id"].as_str().expect("job id").to_string();
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        crate::imports_assets::mapping_request(
            &again_id,
            serde_json::json!({ "mapping": mapping, "options": {} }),
        ),
    )
    .await;
    let checked_again = crate::imports_assets::json(&body);
    assert_eq!(status, StatusCode::OK, "{checked_again}");
    let summary = &checked_again["summary"];
    assert_eq!(summary["rows_with_errors"], 0, "{summary}");
    assert_eq!(summary["would_create"], 3, "{summary}");
    assert_eq!(summary["would_update"], 2, "{summary}");
    assert_eq!(summary["dropped"], serde_json::json!([]), "{summary}");

    commit(&pool, &ws, &again_id).await;
    let outcomes: Vec<(String, String)> = rows(&pool, &ws, &again_id)
        .await
        .iter()
        .map(|r| {
            (
                r["key"].as_str().unwrap_or_default().to_string(),
                r["outcome"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let expect = |key: &str, outcome: &str| (key.to_string(), outcome.to_string());
    assert_eq!(
        outcomes,
        [
            expect("D-1", "create"),
            expect("D-2", "create"),
            expect("B-1", "create"),
            expect("N-2", "update"),
            expect("U-1", "update"),
        ]
    );
    let (status, body) = get(
        &pool,
        &ws,
        &format!("/api/admin/import/{again_id}/error-report"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(report_lines(&body).len(), 1, "{body}");
}
