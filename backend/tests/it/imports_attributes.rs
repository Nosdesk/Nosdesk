//! Kind attributes as columns: cells typed as the row's kind types them,
//! merged into what an asset holds with blanks leaving stored values,
//! checked against the kind's schema (required ones on create), answered
//! per kind at Match values, and dropped and counted where the row's kind
//! has no such attribute; purchase date and primary user; the per-kind
//! template.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text};

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{
    call_import_route, import_workspace, json, mapping_request, upload_request,
};

fn vehicle_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "fuel": { "type": "string", "title": "Fuel", "enum": ["Diesel", "Petrol", "Electric"] },
            "odometer": { "type": "integer", "title": "Odometer", "minimum": 0 },
            "rego": { "type": "string", "title": "Rego" },
            "serviced": { "type": "string", "format": "date", "title": "Last serviced" },
            "insured": { "type": "boolean", "title": "Insured" },
            "condition": { "type": "string", "title": "Condition", "enum": ["Roadworthy", "Unregistered"] },
            "location": { "type": "string", "title": "Depot" },
            "hostname": { "type": "string", "title": "Hostname" },
            "telematics_id": { "type": "string", "title": "Telematics", "synced": true },
            "@tare": { "type": "number", "title": "Tare" }
        },
        "required": ["fuel"]
    })
}

fn camera_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "mount": { "type": "string", "title": "Mount", "enum": ["EF", "PL"] },
            "insured": { "type": "boolean", "title": "Insured" },
            "condition": { "type": "string", "title": "Condition", "enum": ["Good", "Damaged"] }
        }
    })
}

fn seed_kind(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    slug: &str,
    label: &str,
    sort_order: i32,
    schema: serde_json::Value,
) {
    diesel::sql_query(
        "INSERT INTO asset_kinds (slug, label, attribute_schema, sort_order, is_builtin, category, workspace_id)
         VALUES ($1, $2, $3::jsonb, $4, false, 'physical', $5)
         ON CONFLICT (workspace_id, slug) DO UPDATE
         SET label = EXCLUDED.label, attribute_schema = EXCLUDED.attribute_schema,
             sort_order = EXCLUDED.sort_order",
    )
    .bind::<Text, _>(slug)
    .bind::<Text, _>(label)
    .bind::<Text, _>(schema.to_string())
    .bind::<Integer, _>(sort_order)
    .bind::<Integer, _>(ws.workspace_id)
    .execute(&mut pool.get().expect("conn"))
    .expect("seed kind");
}

/// A workspace with Vehicle and Camera kinds and the member's primary
/// email.
fn workspace(pool: &TestPool) -> WorkspaceSeed {
    let ws = import_workspace(pool);
    seed_kind(pool, &ws, "vehicle", "Vehicle", 300, vehicle_schema());
    seed_kind(pool, &ws, "camera", "Camera", 310, camera_schema());
    diesel::sql_query(
        "INSERT INTO user_emails (user_uuid, email, is_primary, is_verified) VALUES ($1, $2, true, true)",
    )
    .bind::<diesel::sql_types::Uuid, _>(ws.member_uuid)
    .bind::<Text, _>(format!("sam-{}@example.com", ws.slug))
    .execute(&mut pool.get().expect("conn"))
    .expect("seed member email");
    ws
}

fn member_email(ws: &WorkspaceSeed) -> String {
    format!("sam-{}@example.com", ws.slug)
}

/// Seed an asset in the workspace.
fn seed_asset(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    tag: &str,
    kind: &str,
    attributes: serde_json::Value,
    synced: Option<&str>,
) {
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, location, attributes, external_sync_source, workspace_id)
         VALUES ($1, $2, $1, 'Depot 1', $3::jsonb, $4, $5)",
    )
    .bind::<Text, _>(tag)
    .bind::<Text, _>(kind)
    .bind::<Text, _>(attributes.to_string())
    .bind::<Nullable<Text>, _>(synced)
    .bind::<Integer, _>(ws.workspace_id)
    .execute(&mut pool.get().expect("conn"))
    .expect("seed asset");
}

#[derive(QueryableByName, Debug)]
struct StoredAsset {
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = Text)]
    attributes: String,
    #[diesel(sql_type = Nullable<Text>)]
    location: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    purchase_date: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    primary_user: Option<String>,
}

fn stored(pool: &TestPool, tag: &str) -> Option<(StoredAsset, serde_json::Value)> {
    diesel::sql_query(
        "SELECT kind, attributes::text AS attributes, location, purchase_date::text AS purchase_date,
                primary_user_uuid::text AS primary_user
         FROM assets WHERE asset_tag = $1",
    )
    .bind::<Text, _>(tag)
    .get_result::<StoredAsset>(&mut pool.get().expect("conn"))
    .optional()
    .expect("read asset")
    .map(|a| {
        let attributes = serde_json::from_str(&a.attributes).expect("attributes json");
        (a, attributes)
    })
}

/// Upload `file` and check it with `mapping` (header name to field id) and
/// `options`: the job.
async fn checked(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    file: &str,
    mapping: &[(&str, &str)],
    options: serde_json::Value,
) -> serde_json::Value {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        upload_request("assets.csv", file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let headers: Vec<&str> = file.lines().next().expect("header").split(',').collect();
    let body_mapping: serde_json::Map<String, serde_json::Value> = mapping
        .iter()
        .map(|(header, field)| {
            let index = headers
                .iter()
                .position(|h| h == header)
                .unwrap_or_else(|| panic!("no {header} column"));
            (field.to_string(), index.into())
        })
        .collect();
    let id = job["id"].as_str().expect("id").to_string();
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        mapping_request(
            &id,
            serde_json::json!({ "mapping": body_mapping, "options": options }),
        ),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    job
}

async fn commit(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> serde_json::Value {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    job
}

async fn rows(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> Vec<serde_json::Value> {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/rows?limit=200")),
    )
    .await;
    let page = json(&body);
    assert_eq!(status, StatusCode::OK, "{page}");
    page["rows"].as_array().expect("rows").clone()
}

fn id_of(job: &serde_json::Value) -> String {
    job["id"].as_str().expect("id").to_string()
}

/// (row, field id, message) for every error of the job's summary.
fn errors(job: &serde_json::Value) -> Vec<(u64, Option<String>, String)> {
    job["summary"]["errors"]
        .as_array()
        .expect("errors")
        .iter()
        .map(|e| {
            (
                e["row"].as_u64().expect("row"),
                e["column"].as_str().map(String::from),
                e["message"].as_str().expect("message").to_string(),
            )
        })
        .collect()
}

/// Number, date (in the Map step's order), yes or no and option cells are
/// typed as the row's kind types them; purchase date and primary user land
/// in their columns; an email that names no member is a row error.
#[actix_web::test]
async fn attribute_cells_are_typed_and_written() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    let file = format!(
        "Name,Type,Tag,Fuel,Odometer,Serviced,Insured,Depot,Purchased,Assigned To\n\
         Van 1,vehicle,V-1,diesel,120500,14/03/2024,Yes,North,02/01/2023,{}\n\
         Van 2,vehicle,V-2,Petrol,,,n,,,nobody@example.com\n",
        member_email(&ws).to_uppercase()
    );
    let job = checked(
        &pool,
        &ws,
        &file,
        &[
            ("Name", "name"),
            ("Type", "kind"),
            ("Tag", "asset_tag"),
            ("Fuel", "fuel"),
            ("Odometer", "odometer"),
            ("Serviced", "serviced"),
            ("Insured", "insured"),
            ("Depot", "attr:location"),
            ("Purchased", "purchase_date"),
            ("Assigned To", "primary_user"),
        ],
        serde_json::json!({ "date_order": "dmy" }),
    )
    .await;
    assert_eq!(
        errors(&job),
        vec![(
            3,
            Some("primary_user".to_string()),
            "'nobody@example.com' is not a member of this workspace".to_string()
        )]
    );
    assert_eq!(
        job["summary"]["errors"][0]["source_column"], "Assigned To",
        "{job}"
    );
    assert_eq!(job["summary"]["would_create"], 1);

    let id = id_of(&job);
    commit(&pool, &ws, &id).await;
    let (asset, attributes) = stored(&pool, "V-1").expect("V-1 created");
    assert_eq!(asset.kind, "vehicle");
    assert_eq!(
        attributes,
        serde_json::json!({
            "fuel": "Diesel",
            "odometer": 120500,
            "serviced": "2024-03-14",
            "insured": true,
            "location": "North"
        })
    );
    // The attribute named like a core column is not the asset's location.
    assert_eq!(asset.location, None);
    assert_eq!(asset.purchase_date.as_deref(), Some("2023-01-02"));
    assert_eq!(asset.primary_user, Some(ws.member_uuid.to_string()));
    assert!(stored(&pool, "V-2").is_none());

    let written = rows(&pool, &ws, &id).await;
    let fields = &written[0]["fields_written"];
    assert_eq!(fields["fuel"]["after"], "Diesel");
    assert_eq!(fields["attr:location"]["after"], "North");
    assert_eq!(fields["primary_user"]["after"], member_email(&ws));
}

/// Attribute cells merge into what an asset holds: a blank cell leaves the
/// stored value, a filled one replaces it. An update that writes no
/// attribute isn't held to the kind's required ones, as PUT isn't; one
/// that does is.
#[actix_web::test]
async fn attributes_merge_and_blank_cells_leave_stored_values() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    seed_asset(
        &pool,
        &ws,
        "V-1",
        "vehicle",
        serde_json::json!({ "fuel": "Petrol", "rego": "ABC-123" }),
        None,
    );
    // Stored before Fuel was required: no fuel.
    seed_asset(&pool, &ws, "V-2", "vehicle", serde_json::json!({}), None);
    seed_asset(&pool, &ws, "V-3", "vehicle", serde_json::json!({}), None);

    let file = "Name,Tag,Fuel,Rego,Odometer\n\
                Van 1,V-1,Diesel,,\n\
                Van 2,V-2,,,\n\
                Van 3,V-3,,XYZ-9,\n";
    let job = checked(
        &pool,
        &ws,
        file,
        &[
            ("Name", "name"),
            ("Tag", "asset_tag"),
            ("Fuel", "fuel"),
            ("Rego", "rego"),
            ("Odometer", "odometer"),
        ],
        serde_json::json!({ "default_kind": "vehicle" }),
    )
    .await;
    assert_eq!(
        errors(&job),
        vec![(
            4,
            Some("fuel".to_string()),
            "required for Vehicle assets".to_string()
        )]
    );
    assert_eq!(job["summary"]["would_update"], 2);

    commit(&pool, &ws, &id_of(&job)).await;
    let (_, v1) = stored(&pool, "V-1").expect("V-1");
    assert_eq!(
        v1,
        serde_json::json!({ "fuel": "Diesel", "rego": "ABC-123" })
    );
    let (_, v3) = stored(&pool, "V-3").expect("V-3");
    assert_eq!(v3, serde_json::json!({}));
}

/// A new asset needs its kind's required attributes; a value set for every
/// row covers one no column feeds (Fuel: Diesel for a Vehicles sheet).
#[actix_web::test]
async fn required_attributes_are_needed_on_create() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    let file = "Name,Tag,Rego\nVan 1,V-1,ABC-1\nVan 2,V-2,ABC-2\n";
    let mapping = [("Name", "name"), ("Tag", "asset_tag"), ("Rego", "rego")];

    let job = checked(
        &pool,
        &ws,
        file,
        &mapping,
        serde_json::json!({ "constants": [{ "field": "kind", "value": "vehicle" }] }),
    )
    .await;
    assert_eq!(job["summary"]["would_create"], 0);
    let groups = &job["summary"]["error_groups"];
    assert_eq!(groups[0]["column"], "fuel", "{job}");
    assert_eq!(groups[0]["message"], "required for Vehicle assets");
    assert_eq!(groups[0]["count"], 2);
    assert!(groups[0]["source_column"].is_null());

    let job = checked(
        &pool,
        &ws,
        file,
        &mapping,
        serde_json::json!({ "constants": [
            { "field": "kind", "value": "vehicle" },
            { "field": "fuel", "value": "Diesel" },
        ] }),
    )
    .await;
    assert_eq!(errors(&job), vec![]);
    assert_eq!(job["summary"]["would_create"], 2);
    commit(&pool, &ws, &id_of(&job)).await;
    let (_, attributes) = stored(&pool, "V-2").expect("V-2");
    assert_eq!(
        attributes,
        serde_json::json!({ "fuel": "Diesel", "rego": "ABC-2" })
    );
}

/// A value for an attribute the row's kind doesn't have is left out and
/// counted by field and kind; the row imports. A value set for every row
/// isn't counted, since the file didn't hold it.
#[actix_web::test]
async fn an_attribute_the_kind_lacks_is_dropped_and_counted() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    let file = "Name,Type,Tag,Rego,Mount\n\
                Cam 1,camera,C-1,ABC-1,EF\n\
                Cam 2,camera,C-2,ABC-2,\n\
                Van 1,vehicle,V-1,ABC-3,PL\n";
    let job = checked(
        &pool,
        &ws,
        file,
        &[
            ("Name", "name"),
            ("Type", "kind"),
            ("Tag", "asset_tag"),
            ("Rego", "rego"),
            ("Mount", "mount"),
        ],
        serde_json::json!({ "constants": [{ "field": "fuel", "value": "Diesel" }] }),
    )
    .await;
    assert_eq!(errors(&job), vec![]);
    assert_eq!(job["summary"]["would_create"], 3);
    assert_eq!(
        job["summary"]["dropped"],
        serde_json::json!([
            { "column": "rego", "source_column": "Rego", "kind": "camera", "count": 2 },
            { "column": "mount", "source_column": "Mount", "kind": "vehicle", "count": 1 },
        ])
    );
    let id = id_of(&job);
    let checked_rows = rows(&pool, &ws, &id).await;
    assert_eq!(checked_rows[0]["outcome"], "create");
    assert_eq!(
        checked_rows[0]["dropped"],
        serde_json::json!([{ "column": "rego", "source_column": "Rego", "kind": "camera" }])
    );

    commit(&pool, &ws, &id).await;
    let (_, camera) = stored(&pool, "C-1").expect("C-1");
    assert_eq!(camera, serde_json::json!({ "mount": "EF" }));
    let (_, van) = stored(&pool, "V-1").expect("V-1");
    assert_eq!(
        van,
        serde_json::json!({ "fuel": "Diesel", "rego": "ABC-3" })
    );
}

/// Option and yes-or-no attribute values are matched against the options
/// of the kind each row has, after the kind's own answers: the same text
/// can be exact for one kind and unmatched for another, and an answer is
/// given per kind.
#[actix_web::test]
async fn option_and_boolean_values_are_answered_per_kind() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    let file = "Name,Type,Tag,Condition,Insured\n\
                Cam 1,cam,C-1,good,maybe\n\
                Van 1,vehicle,V-1,good,maybe\n\
                Van 2,vehicle,V-2,roadworthy,y\n";
    let job = checked(
        &pool,
        &ws,
        file,
        &[
            ("Name", "name"),
            ("Type", "kind"),
            ("Tag", "asset_tag"),
            ("Condition", "condition"),
            ("Insured", "insured"),
        ],
        serde_json::json!({ "constants": [{ "field": "fuel", "value": "Diesel" }] }),
    )
    .await;
    let id = id_of(&job);

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/values")),
    )
    .await;
    let view = json(&body);
    assert_eq!(status, StatusCode::OK, "{view}");
    let column = |field: &str, context: &str| {
        view["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .find(|c| c["field"] == field && c["context"] == context)
            .unwrap_or_else(|| panic!("no {field}/{context} column in {view}"))
            .clone()
    };
    let value = |column: &serde_json::Value, text: &str| {
        column["values"]
            .as_array()
            .expect("values")
            .iter()
            .find(|v| v["text"] == text)
            .unwrap_or_else(|| panic!("no {text} in {column}"))
            .clone()
    };
    // Cam was guessed as Camera, so its row's Condition reads as a camera's.
    let camera = column("condition", "camera");
    assert_eq!(camera["context_label"], "Camera");
    assert_eq!(value(&camera, "good")["match"], "exact");
    let vehicle = column("condition", "vehicle");
    assert_eq!(value(&vehicle, "good")["match"], "no_match");
    assert_eq!(
        value(&vehicle, "good")["suggested"],
        serde_json::json!({ "action": "leave_blank" })
    );
    let insured = column("insured", "vehicle");
    assert_eq!(value(&insured, "maybe")["match"], "no_match");
    assert_eq!(
        value(&insured, "maybe")["suggested"],
        serde_json::json!({ "action": "skip" })
    );
    // Rows the last check skipped for "maybe": one camera, one vehicle.
    assert_eq!(job["summary"]["rows_with_errors"], 2, "{job}");

    // Answers outside the field's kinds are refused.
    for (bad, expected) in [
        (
            serde_json::json!({ "condition": { "": { "good": { "action": "skip" } } } }),
            "per kind",
        ),
        (
            serde_json::json!({ "condition": { "laptop": { "good": { "action": "skip" } } } }),
            "per kind",
        ),
        (
            serde_json::json!({ "condition": { "vehicle": { "good": { "action": "match", "target": "Good" } } } }),
            "isn't one of the choices",
        ),
    ] {
        let (status, body) = call_import_route(
            &pool,
            &ws,
            ws.admin_uuid,
            TestRequest::put()
                .uri(&format!("/api/admin/import/{id}/values"))
                .set_json(serde_json::json!({ "answers": bad })),
        )
        .await;
        let body = json(&body);
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body["error"].as_str().unwrap_or("").contains(expected),
            "{expected}: {body}"
        );
    }

    let answers = serde_json::json!({
        "condition": { "vehicle": { "good": { "action": "match", "target": "Roadworthy" } } },
        "insured": {
            "vehicle": { "maybe": { "action": "match", "target": "true" } },
            "camera": { "maybe": { "action": "leave_blank" } },
        },
    });
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::put()
            .uri(&format!("/api/admin/import/{id}/values"))
            .set_json(serde_json::json!({ "answers": answers })),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(errors(&job), vec![]);
    assert_eq!(job["summary"]["would_create"], 3);

    commit(&pool, &ws, &id).await;
    let (_, cam) = stored(&pool, "C-1").expect("C-1");
    assert_eq!(cam, serde_json::json!({ "condition": "Good" }));
    let (_, van) = stored(&pool, "V-1").expect("V-1");
    assert_eq!(
        van,
        serde_json::json!({ "fuel": "Diesel", "condition": "Roadworthy", "insured": true })
    );
    let (_, van) = stored(&pool, "V-2").expect("V-2");
    assert_eq!(
        van,
        serde_json::json!({ "fuel": "Diesel", "condition": "Roadworthy", "insured": true })
    );
}

/// A number or date attribute cell that doesn't read is a value to answer
/// on its kind's card, skipped by default; Replace with fixes it.
#[actix_web::test]
async fn unreadable_attribute_values_take_a_replacement() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    let file = "Name,Tag,Odometer,Serviced\n\
                Van 1,V-1,lots,31/02/2023\n\
                Van 2,V-2,1200,01/02/2023\n";
    let job = checked(
        &pool,
        &ws,
        file,
        &[
            ("Name", "name"),
            ("Tag", "asset_tag"),
            ("Odometer", "odometer"),
            ("Serviced", "serviced"),
        ],
        serde_json::json!({
            "date_order": "dmy",
            "constants": [
                { "field": "kind", "value": "vehicle" },
                { "field": "fuel", "value": "Petrol" },
            ],
        }),
    )
    .await;
    let mut found = errors(&job);
    found.sort();
    assert_eq!(
        found,
        vec![
            (
                2,
                Some("odometer".to_string()),
                "'lots' is not a valid decimal".to_string()
            ),
            (
                2,
                Some("serviced".to_string()),
                "'31/02/2023' is not a valid date; use DD/MM/YYYY".to_string()
            ),
        ]
    );

    let id = id_of(&job);
    let answers = serde_json::json!({
        "odometer": { "vehicle": { "lots": { "action": "replace", "value": "99000" } } },
        "serviced": { "vehicle": { "31/02/2023": { "action": "replace", "value": "28/02/2023" } } },
    });
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::put()
            .uri(&format!("/api/admin/import/{id}/values"))
            .set_json(serde_json::json!({ "answers": answers })),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(errors(&job), vec![]);
    commit(&pool, &ws, &id).await;
    let (_, van) = stored(&pool, "V-1").expect("V-1");
    assert_eq!(
        van,
        serde_json::json!({ "fuel": "Petrol", "odometer": 99000, "serviced": "2023-02-28" })
    );
}

/// An asset synced from Microsoft takes attributes from the file, as PUT
/// lets the asset page change them, and no other column.
#[actix_web::test]
async fn a_synced_asset_takes_attributes_only() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    seed_asset(
        &pool,
        &ws,
        "V-1",
        "vehicle",
        serde_json::json!({ "fuel": "Petrol", "hostname": "VAN-1" }),
        Some("intune"),
    );
    seed_asset(
        &pool,
        &ws,
        "V-2",
        "vehicle",
        serde_json::json!({ "fuel": "Petrol" }),
        Some("intune"),
    );
    seed_asset(
        &pool,
        &ws,
        "V-3",
        "vehicle",
        serde_json::json!({ "fuel": "Petrol" }),
        Some("intune"),
    );
    let file = "Name,Tag,Rego,Location\n\
                V-1,V-1,ABC-1,\n\
                V-2,V-2,,Depot 1\n\
                V-3,V-3,,Depot 2\n";
    let job = checked(
        &pool,
        &ws,
        file,
        &[
            ("Name", "name"),
            ("Tag", "asset_tag"),
            ("Rego", "rego"),
            ("Location", "location"),
        ],
        serde_json::json!({ "default_kind": "vehicle" }),
    )
    .await;
    assert_eq!(
        errors(&job),
        vec![(
            4,
            Some("location".to_string()),
            "V-3 is synced from Microsoft Intune, which sets its location; unmanage it on the asset page to edit it here".to_string()
        )]
    );
    assert_eq!(job["summary"]["would_update"], 1);
    assert_eq!(job["summary"]["unchanged"], 1);

    commit(&pool, &ws, &id_of(&job)).await;
    let (asset, attributes) = stored(&pool, "V-1").expect("V-1");
    assert_eq!(
        attributes,
        serde_json::json!({ "fuel": "Petrol", "hostname": "VAN-1", "rego": "ABC-1" })
    );
    assert_eq!(asset.location.as_deref(), Some("Depot 1"));
}

/// The template for a kind lists the core columns, then the kind's own
/// attributes sorted by title, leaving out the ones a sync feeds; a header
/// that reads as a formula is guarded. An unknown kind is refused.
#[actix_web::test]
async fn the_template_lists_a_kinds_attributes() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import/template/assets?kind=vehicle"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8_lossy(&body);
    assert_eq!(
        text,
        "\u{FEFF}name,kind,asset_tag,serial_number,status,manufacturer,model,location,primary_user,\
         purchase_date,groups,notes,quantity,unit,low_stock_threshold,\
         condition,attr:location,fuel,insured,serviced,odometer,rego,'@tare\n"
    );

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import/template/assets?kind=camera"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        String::from_utf8_lossy(&body).ends_with(",low_stock_threshold,condition,insured,mount\n")
    );

    for uri in [
        "/api/admin/import/template/assets?kind=boat",
        "/api/admin/import/template/users?kind=vehicle",
    ] {
        let (status, _) =
            call_import_route(&pool, &ws, ws.admin_uuid, TestRequest::get().uri(uri)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
    }

    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.member_uuid,
        TestRequest::get().uri("/api/admin/import/template/assets?kind=vehicle"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Attributes are fields of their own: the key, or `attr:<key>` beside a
/// core column; typed by the schema; with the kinds that define them and
/// those that require them. Sync-owned and synced keys are left out.
#[actix_web::test]
async fn attributes_are_fields_with_their_kinds() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool);
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import/fields?type=assets"),
    )
    .await;
    let body = json(&body);
    assert_eq!(status, StatusCode::OK, "{body}");
    let fields = body["fields"].as_array().expect("fields");
    let field = |id: &str| {
        fields
            .iter()
            .find(|f| f["id"] == id)
            .unwrap_or_else(|| panic!("no field {id}"))
            .clone()
    };
    let fuel = field("fuel");
    assert_eq!(fuel["group"], "attribute");
    assert_eq!(fuel["label"], "Fuel");
    assert_eq!(fuel["value_type"], "enum");
    assert_eq!(fuel["required"], false);
    assert_eq!(fuel["kinds"], serde_json::json!(["vehicle"]));
    assert_eq!(fuel["required_for"], serde_json::json!(["vehicle"]));
    assert_eq!(field("odometer")["value_type"], "number");
    assert_eq!(field("serviced")["value_type"], "date");
    assert_eq!(field("insured")["value_type"], "boolean");
    assert_eq!(
        field("insured")["kinds"],
        serde_json::json!(["vehicle", "camera"])
    );
    assert_eq!(field("attr:location")["label"], "Depot");
    // Both kinds' options, for a value set for every row.
    assert_eq!(
        field("condition")["enum"],
        serde_json::json!(["Roadworthy", "Unregistered", "Good", "Damaged"])
    );
    assert!(!fields.iter().any(|f| f["id"] == "hostname"));
    assert!(!fields.iter().any(|f| f["id"] == "telematics_id"));
}
