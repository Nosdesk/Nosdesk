//! Catalog and groups: manufacturers and models matched ignoring case and
//! stamped onto assets, names the catalog holds twice refused, entries
//! created where Match values says so (and only by the commit), groups
//! added to what an asset is in, directory groups left off, and locations
//! snapped to a stored spelling.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text};

use crate::common::{TestPool, WorkspaceSeed};
use crate::imports_assets::{call_import_route, import_workspace, json};
use crate::imports_attributes::{checked, commit, errors, id_of, rows, seed_kind};

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = Integer)]
    id: i32,
}

#[derive(QueryableByName, Debug)]
struct CountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    n: i64,
}

fn camera_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "mount": { "type": "string", "title": "Mount", "enum": ["E", "PL", "EF"] },
            "sensor": { "type": "string", "title": "Sensor" }
        }
    })
}

fn vehicle_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": { "fuel": { "type": "string", "title": "Fuel" } },
        "required": ["fuel"]
    })
}

fn insert_id(pool: &TestPool, sql: &str, ws: &WorkspaceSeed, name: &str) -> i32 {
    diesel::sql_query(sql)
        .bind::<Text, _>(name)
        .bind::<Integer, _>(ws.workspace_id)
        .get_result::<IdRow>(&mut pool.get().expect("conn"))
        .expect("seed")
        .id
}

fn seed_maker(pool: &TestPool, ws: &WorkspaceSeed, name: &str) -> i32 {
    insert_id(
        pool,
        "INSERT INTO manufacturers (name, workspace_id) VALUES ($1, $2) RETURNING id",
        ws,
        name,
    )
}

fn seed_model(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    maker: i32,
    name: &str,
    kind: &str,
    defaults: serde_json::Value,
) -> i32 {
    diesel::sql_query(
        "INSERT INTO asset_models (manufacturer_id, name, kind, default_attributes, workspace_id)
         VALUES ($1, $2, $3, $4::jsonb, $5) RETURNING id",
    )
    .bind::<Integer, _>(maker)
    .bind::<Text, _>(name)
    .bind::<Text, _>(kind)
    .bind::<Text, _>(defaults.to_string())
    .bind::<Integer, _>(ws.workspace_id)
    .get_result::<IdRow>(&mut pool.get().expect("conn"))
    .expect("seed model")
    .id
}

fn seed_group(pool: &TestPool, ws: &WorkspaceSeed, name: &str) -> i32 {
    insert_id(
        pool,
        "INSERT INTO asset_groups (name, workspace_id) VALUES ($1, $2) RETURNING id",
        ws,
        name,
    )
}

fn seed_directory_group(pool: &TestPool, ws: &WorkspaceSeed, name: &str) -> i32 {
    insert_id(
        pool,
        "INSERT INTO groups (name, external_source, workspace_id) VALUES ($1, 'entra', $2) RETURNING id",
        ws,
        name,
    )
}

/// An asset with `tag`, optionally linked to a model and synced.
fn seed_asset(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    tag: &str,
    kind: &str,
    location: Option<&str>,
    model: Option<(i32, &str, &str)>,
    synced: bool,
) -> i32 {
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, location, model_id, manufacturer, model,
                             external_sync_source, workspace_id)
         VALUES ($1, $2, $1, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind::<Text, _>(tag)
    .bind::<Text, _>(kind)
    .bind::<Nullable<Text>, _>(location)
    .bind::<Nullable<Integer>, _>(model.map(|m| m.0))
    .bind::<Nullable<Text>, _>(model.map(|m| m.1))
    .bind::<Nullable<Text>, _>(model.map(|m| m.2))
    .bind::<Nullable<Text>, _>(synced.then_some("intune"))
    .bind::<Integer, _>(ws.workspace_id)
    .get_result::<IdRow>(&mut pool.get().expect("conn"))
    .expect("seed asset")
    .id
}

fn add_to_group(pool: &TestPool, ws: &WorkspaceSeed, asset: i32, group: i32) {
    diesel::sql_query(
        "INSERT INTO asset_group_assignments (group_id, asset_id, workspace_id) VALUES ($1, $2, $3)",
    )
    .bind::<Integer, _>(group)
    .bind::<Integer, _>(asset)
    .bind::<Integer, _>(ws.workspace_id)
    .execute(&mut pool.get().expect("conn"))
    .expect("assign group");
}

/// The fixture: Camera and Vehicle kinds; Sony and Canon with their
/// models; groups Studio A and Field kit; a directory group.
struct Catalog {
    sony: i32,
    fx6: i32,
    z280: i32,
    c300: i32,
    studio_a: i32,
    field_kit: i32,
}

fn workspace(pool: &TestPool) -> (WorkspaceSeed, Catalog) {
    let ws = import_workspace(pool);
    seed_kind(pool, &ws, "camera", "Camera", 300, camera_schema());
    seed_kind(pool, &ws, "vehicle", "Vehicle", 310, vehicle_schema());
    let sony = seed_maker(pool, &ws, "Sony");
    let canon = seed_maker(pool, &ws, "Canon");
    let fx6 = seed_model(
        pool,
        &ws,
        sony,
        "FX6",
        "camera",
        serde_json::json!({ "mount": "E", "sensor": "Full frame" }),
    );
    let z280 = seed_model(pool, &ws, sony, "PXW-Z280", "camera", serde_json::json!({}));
    let c300 = seed_model(pool, &ws, canon, "C300", "camera", serde_json::json!({}));
    let studio_a = seed_group(pool, &ws, "Studio A");
    let field_kit = seed_group(pool, &ws, "Field kit");
    seed_directory_group(pool, &ws, "Sales Laptops");
    (
        ws,
        Catalog {
            sony,
            fx6,
            z280,
            c300,
            studio_a,
            field_kit,
        },
    )
}

#[derive(QueryableByName, Debug)]
struct Stored {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = Nullable<Text>)]
    manufacturer: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    model: Option<String>,
    #[diesel(sql_type = Nullable<Integer>)]
    model_id: Option<i32>,
    #[diesel(sql_type = Nullable<Text>)]
    location: Option<String>,
    #[diesel(sql_type = Text)]
    attributes: String,
}

fn stored(pool: &TestPool, tag: &str) -> Option<Stored> {
    diesel::sql_query(
        "SELECT id, kind, manufacturer, model, model_id, location, attributes::text AS attributes
         FROM assets WHERE asset_tag = $1",
    )
    .bind::<Text, _>(tag)
    .get_result::<Stored>(&mut pool.get().expect("conn"))
    .optional()
    .expect("read asset")
}

fn attributes(asset: &Stored) -> serde_json::Value {
    serde_json::from_str(&asset.attributes).expect("attributes json")
}

fn group_names(pool: &TestPool, asset: i32) -> Vec<String> {
    #[derive(QueryableByName)]
    struct Name {
        #[diesel(sql_type = Text)]
        name: String,
    }
    diesel::sql_query(
        "SELECT g.name FROM asset_group_assignments a JOIN asset_groups g ON g.id = a.group_id
         WHERE a.asset_id = $1 ORDER BY g.name",
    )
    .bind::<Integer, _>(asset)
    .load::<Name>(&mut pool.get().expect("conn"))
    .expect("read groups")
    .into_iter()
    .map(|n| n.name)
    .collect()
}

fn count(pool: &TestPool, sql: &str) -> i64 {
    diesel::sql_query(sql)
        .get_result::<CountRow>(&mut pool.get().expect("conn"))
        .expect("count")
        .n
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

/// The view's column for `field` in `context`.
fn column<'a>(view: &'a serde_json::Value, field: &str, context: &str) -> &'a serde_json::Value {
    view["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|c| c["field"] == field && c["context"] == context)
        .unwrap_or_else(|| panic!("no {field} column for '{context}' in {view}"))
}

fn value<'a>(column: &'a serde_json::Value, text: &str) -> &'a serde_json::Value {
    column["values"]
        .as_array()
        .expect("values")
        .iter()
        .find(|v| v["text"] == text)
        .unwrap_or_else(|| panic!("no value '{text}' in {column}"))
}

const CAMERA_MAPPING: &[(&str, &str)] = &[
    ("Name", "name"),
    ("Type", "kind"),
    ("Tag", "asset_tag"),
    ("Make", "manufacturer"),
    ("Model", "model"),
    ("Mount", "mount"),
];

/// Manufacturers match by name ignoring case, models within the row's
/// manufacturer, or across the catalog when the row has none. A linked
/// model stamps its manufacturer and name, its kind where the kind cell is
/// blank, its defaults where the asset has no value, and the link. A model
/// nobody has is kept as text.
#[actix_web::test]
async fn models_match_within_their_manufacturer_and_stamp_the_asset() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ws, cat) = workspace(&pool);
    let file = "Name,Type,Tag,Make,Model,Mount\n\
                Cam 1,,C-1,sony,fx6,\n\
                Cam 2,camera,C-2,SONY ,pxw-z280,PL\n\
                Cam 3,,C-3,,c300,ef\n\
                Cam 4,camera,C-4,Sony,URSA,\n";
    let job = checked(&pool, &ws, file, CAMERA_MAPPING, serde_json::json!({})).await;
    assert_eq!(errors(&job), vec![], "{job}");
    assert_eq!(job["summary"]["would_create"], 4);
    let id = id_of(&job);

    // The Mount values of Cam 3 are answered as a camera's: its kind comes
    // from its model.
    let view = get_values(&pool, &ws, &id).await;
    let mount = column(&view, "mount", "camera");
    assert_eq!(value(mount, "ef")["rows"], serde_json::json!([4]));
    // Models are answered per manufacturer: Sony's, and the catalog's for
    // the row without one.
    let sony_models = column(&view, "model", "sony");
    assert_eq!(sony_models["context_label"], "Sony");
    assert_eq!(sony_models["lookup"], "model");
    assert_eq!(value(sony_models, "fx6")["match"], "exact");
    assert_eq!(value(sony_models, "URSA")["match"], "no_match");
    assert_eq!(
        value(sony_models, "URSA")["suggested"],
        serde_json::json!({ "action": "keep_text" })
    );
    assert_eq!(value(column(&view, "model", ""), "c300")["match"], "exact");

    commit(&pool, &ws, &id).await;
    let c1 = stored(&pool, "C-1").expect("C-1");
    assert_eq!(c1.kind, "camera");
    assert_eq!(c1.manufacturer.as_deref(), Some("Sony"));
    assert_eq!(c1.model.as_deref(), Some("FX6"));
    assert_eq!(c1.model_id, Some(cat.fx6));
    assert_eq!(
        attributes(&c1),
        serde_json::json!({ "mount": "E", "sensor": "Full frame" })
    );
    let c2 = stored(&pool, "C-2").expect("C-2");
    assert_eq!(c2.model_id, Some(cat.z280));
    assert_eq!(c2.manufacturer.as_deref(), Some("Sony"));
    assert_eq!(attributes(&c2), serde_json::json!({ "mount": "PL" }));
    let c3 = stored(&pool, "C-3").expect("C-3");
    assert_eq!(c3.model_id, Some(cat.c300));
    assert_eq!(c3.manufacturer.as_deref(), Some("Canon"));
    assert_eq!(c3.kind, "camera");
    assert_eq!(attributes(&c3), serde_json::json!({ "mount": "EF" }));
    let c4 = stored(&pool, "C-4").expect("C-4");
    assert_eq!(c4.model_id, None);
    assert_eq!(c4.model.as_deref(), Some("URSA"));
    assert_eq!(c4.manufacturer.as_deref(), Some("Sony"));
}

/// A name the catalog holds twice, told apart only by case, links to
/// neither, and a model name more than one manufacturer has needs the
/// row's manufacturer; each row is skipped. A kind cell must agree with the
/// model.
#[actix_web::test]
async fn ambiguous_names_and_a_disagreeing_kind_are_row_errors() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ws, _) = workspace(&pool);
    seed_maker(&pool, &ws, "SONY");
    let canon = seed_maker(&pool, &ws, "Canon Inc");
    seed_model(&pool, &ws, canon, "FX6", "camera", serde_json::json!({}));
    let file = "Name,Type,Tag,Make,Model,Mount\n\
                A,camera,A-1,sony,,\n\
                B,camera,B-1,,fx6,\n\
                C,generic,C-1,Canon,C300,\n";
    let job = checked(&pool, &ws, file, CAMERA_MAPPING, serde_json::json!({})).await;
    let errs = errors(&job);
    assert_eq!(errs.len(), 3, "{job}");
    assert_eq!(errs[0].0, 2);
    assert_eq!(errs[0].1.as_deref(), Some("manufacturer"));
    assert!(
        errs[0]
            .2
            .starts_with("'sony' matches more than one manufacturer:"),
        "{}",
        errs[0].2
    );
    assert_eq!(
        errs[1],
        (
            3,
            Some("model".to_string()),
            "'fx6' is a model of more than one manufacturer (Canon Inc, Sony); give the row a manufacturer".to_string()
        )
    );
    assert_eq!(
        errs[2],
        (
            4,
            Some("kind".to_string()),
            "the model C300 is a Camera, not a Generic asset".to_string()
        )
    );
    assert_eq!(job["summary"]["would_create"], 0);

    // Match values says why and skips the rows until a manufacturer is
    // picked.
    let view = get_values(&pool, &ws, &id_of(&job)).await;
    let sony = value(column(&view, "manufacturer", ""), "sony");
    assert_eq!(sony["match"], "no_match");
    assert_eq!(sony["reason"]["code"], "ambiguous");
    assert_eq!(sony["suggested"], serde_json::json!({ "action": "skip" }));
}

/// Values no entry has are created where Match values says so, inside the
/// commit and through the repositories; the dry run lists them and writes
/// nothing. A model takes the row's kind and needs a manufacturer, and a
/// kind whose new models need attribute values can't take one.
#[actix_web::test]
async fn create_answers_create_within_the_commit() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ws, cat) = workspace(&pool);
    let file = "Name,Type,Tag,Make,Model,Groups\n\
                Cam 1,camera,N-1,Blackmagic,URSA Mini,Studio A;New Room\n\
                Cam 2,camera,N-2,blackmagic,ursa mini,new room\n\
                Van,vehicle,N-3,Ford,Transit,\n\
                Cam 3,camera,N-4,,Pocket 6K,\n";
    let mapping: &[(&str, &str)] = &[
        ("Name", "name"),
        ("Type", "kind"),
        ("Tag", "asset_tag"),
        ("Make", "manufacturer"),
        ("Model", "model"),
        ("Groups", "groups"),
    ];
    let job = checked(&pool, &ws, file, mapping, serde_json::json!({})).await;
    let id = id_of(&job);

    // Unknown names are kept as text and unknown groups left off until
    // answered otherwise.
    let view = get_values(&pool, &ws, &id).await;
    let makers = column(&view, "manufacturer", "");
    assert_eq!(makers["lookup"], "manufacturer");
    assert_eq!(
        value(makers, "Blackmagic")["suggested"],
        serde_json::json!({ "action": "keep_text" })
    );
    let groups = column(&view, "groups", "");
    assert_eq!(value(groups, "Studio A")["match"], "exact");
    assert_eq!(value(groups, "New Room")["rows"], serde_json::json!([2, 3]));
    assert_eq!(
        value(groups, "New Room")["suggested"],
        serde_json::json!({ "action": "leave_blank" })
    );
    assert!(column(&view, "model", "blackmagic")["choices"]
        .as_array()
        .is_some_and(Vec::is_empty));

    // Creating what exists, or keeping a group as text, doesn't fit.
    let (status, body) = put_values(
        &pool,
        &ws,
        &id,
        serde_json::json!({ "manufacturer": { "": { "SONY": { "action": "create" } } } }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = put_values(
        &pool,
        &ws,
        &id,
        serde_json::json!({ "groups": { "": { "New Room": { "action": "keep_text" } } } }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let create = serde_json::json!({ "action": "create" });
    let (status, job) = put_values(
        &pool,
        &ws,
        &id,
        serde_json::json!({
            "manufacturer": { "": { "Blackmagic": create, "Ford": create } },
            "model": {
                "blackmagic": { "URSA Mini": create },
                "ford": { "Transit": create },
                "": { "Pocket 6K": create }
            },
            "groups": { "": { "New Room": create } }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(
        job["summary"]["catalog"],
        serde_json::json!({
            "manufacturers": ["Blackmagic"],
            "models": [{ "manufacturer": "Blackmagic", "name": "URSA Mini", "kind": "camera" }],
            "groups": ["New Room"]
        }),
        "{job}"
    );
    let errs = errors(&job);
    assert!(errs.contains(&(
        4,
        Some("model".to_string()),
        "'Transit' can't be created here: new Vehicle models need Fuel; add it under Asset models first"
            .to_string()
    )));
    assert!(errs.contains(&(
        5,
        Some("model".to_string()),
        "'Pocket 6K' needs a manufacturer".to_string()
    )));
    // The dry run wrote nothing.
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) AS n FROM manufacturers WHERE name ILIKE 'blackmagic'"
        ),
        0
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) AS n FROM asset_groups WHERE name = 'New Room'"
        ),
        0
    );
    assert!(stored(&pool, "N-1").is_none());

    commit(&pool, &ws, &id).await;
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) AS n FROM manufacturers WHERE name = 'Blackmagic'"
        ),
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) AS n FROM manufacturers WHERE name = 'Ford'"
        ),
        0,
        "Ford's only row was skipped"
    );
    let n1 = stored(&pool, "N-1").expect("N-1");
    let n2 = stored(&pool, "N-2").expect("N-2");
    assert_eq!(n1.manufacturer.as_deref(), Some("Blackmagic"));
    assert_eq!(n2.manufacturer.as_deref(), Some("Blackmagic"));
    assert_eq!(n1.model.as_deref(), Some("URSA Mini"));
    assert_eq!(n2.model.as_deref(), Some("URSA Mini"));
    assert!(n1.model_id.is_some());
    assert_eq!(n1.model_id, n2.model_id);
    assert_eq!(group_names(&pool, n1.id), vec!["New Room", "Studio A"]);
    assert_eq!(group_names(&pool, n2.id), vec!["New Room"]);

    // The first row that needed each entry records it.
    let written = rows(&pool, &ws, &id).await;
    let kinds: Vec<&str> = written[0]["catalog_created"]
        .as_array()
        .expect("catalog_created")
        .iter()
        .map(|c| c["kind"].as_str().expect("kind"))
        .collect();
    assert_eq!(kinds, vec!["manufacturer", "model", "group"]);
    assert!(
        written[1].get("catalog_created").is_none(),
        "{}",
        written[1]
    );
    let added: Vec<i64> = written[0]["groups_added"]
        .as_array()
        .expect("groups_added")
        .iter()
        .filter_map(serde_json::Value::as_i64)
        .collect();
    assert!(added.contains(&i64::from(cat.studio_a)));
    assert_eq!(added.len(), 2);
}

/// Rows creating one model under different kinds are each skipped, naming
/// the rows.
#[actix_web::test]
async fn a_model_created_as_two_kinds_skips_its_rows() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ws, _) = workspace(&pool);
    let file = "Name,Type,Tag,Make,Model,Mount\n\
                A,camera,K-1,Sony,Venice,\n\
                B,generic,K-2,Sony,venice,\n\
                C,camera,K-3,Sony,Venice,\n";
    let job = checked(&pool, &ws, file, CAMERA_MAPPING, serde_json::json!({})).await;
    let create = serde_json::json!({ "action": "create" });
    let (status, job) = put_values(
        &pool,
        &ws,
        &id_of(&job),
        serde_json::json!({ "model": { "sony": { "Venice": create } } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let message = "'Venice' would be created as more than one kind: Camera (rows 2 and 4), Generic asset (row 3); give these rows one kind";
    assert_eq!(
        errors(&job),
        vec![
            (2, Some("model".to_string()), message.to_string()),
            (
                3,
                Some("model".to_string()),
                message.replace("'Venice'", "'venice'")
            ),
            (4, Some("model".to_string()), message.to_string()),
        ]
    );
    assert!(job["summary"].get("catalog").is_none(), "{job}");
}

/// Groups add to what an asset is in. A name no active group has, and a
/// directory group's, are left off and counted; the import never writes a
/// directory membership.
#[actix_web::test]
async fn groups_add_to_an_assets_groups_and_directory_groups_are_left_off() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ws, cat) = workspace(&pool);
    let asset = seed_asset(&pool, &ws, "G-1", "generic", None, None, false);
    add_to_group(&pool, &ws, asset, cat.field_kit);
    let file = "Name,Tag,Groups\n\
                Desk,G-1,studio a; Sales Laptops; Spare Parts\n";
    let mapping: &[(&str, &str)] = &[("Name", "name"), ("Tag", "asset_tag"), ("Groups", "groups")];
    let job = checked(
        &pool,
        &ws,
        file,
        mapping,
        serde_json::json!({ "default_kind": "generic" }),
    )
    .await;
    assert_eq!(errors(&job), vec![], "{job}");
    assert_eq!(job["summary"]["would_update"], 1);
    let dropped: Vec<&str> = job["summary"]["dropped"]
        .as_array()
        .expect("dropped")
        .iter()
        .filter(|d| d["column"] == "groups")
        .filter_map(|d| d["value"].as_str())
        .collect();
    assert_eq!(dropped, vec!["Sales Laptops", "Spare Parts"]);

    let id = id_of(&job);
    let view = get_values(&pool, &ws, &id).await;
    let groups = column(&view, "groups", "");
    let directory = value(groups, "Sales Laptops");
    assert_eq!(directory["reason"]["code"], "disabled");
    assert_eq!(
        directory["reason"]["note"],
        "Directory groups are set by directory sync"
    );

    commit(&pool, &ws, &id).await;
    assert_eq!(group_names(&pool, asset), vec!["Field kit", "Studio A"]);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) AS n FROM asset_directory_memberships m
             JOIN assets a ON a.id = m.asset_id WHERE a.asset_tag = 'G-1'"
        ),
        0
    );
    let written = rows(&pool, &ws, &id).await;
    assert_eq!(
        written[0]["groups_added"],
        serde_json::json!([cat.studio_a])
    );
}

/// A location that differs from a stored spelling only by case or spacing
/// takes that spelling, and the check counts how many did.
#[actix_web::test]
async fn locations_snap_to_a_stored_spelling() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ws, _) = workspace(&pool);
    seed_asset(&pool, &ws, "L-0", "generic", Some("Studio A"), None, false);
    let file = "Name,Type,Tag,Room\n\
                One,generic,L-1,studio a\n\
                Two,generic,L-2,Studio   A\n\
                Three,generic,L-3,Studio A\n\
                Four,generic,L-4,Studio B\n";
    let mapping: &[(&str, &str)] = &[
        ("Name", "name"),
        ("Type", "kind"),
        ("Tag", "asset_tag"),
        ("Room", "location"),
    ];
    let job = checked(&pool, &ws, file, mapping, serde_json::json!({})).await;
    let notices = job["summary"]["notices"].as_array().expect("notices");
    let snapped = notices
        .iter()
        .find(|n| n["code"] == "location_snapped")
        .unwrap_or_else(|| panic!("no snap notice in {job}"));
    assert_eq!(snapped["count"], 2);
    assert_eq!(
        snapped["message"],
        "2 locations matched an existing spelling."
    );

    commit(&pool, &ws, &id_of(&job)).await;
    let location = |tag: &str| stored(&pool, tag).expect(tag).location;
    assert_eq!(location("L-1").as_deref(), Some("Studio A"));
    assert_eq!(location("L-2").as_deref(), Some("Studio A"));
    assert_eq!(location("L-3").as_deref(), Some("Studio A"));
    assert_eq!(location("L-4").as_deref(), Some("Studio B"));
}

/// A synced asset can't take another model, but its row may name the
/// model it has, in any case, and add it to groups.
#[actix_web::test]
async fn a_synced_asset_keeps_its_model() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ws, cat) = workspace(&pool);
    seed_asset(
        &pool,
        &ws,
        "S-1",
        "camera",
        None,
        Some((cat.fx6, "Sony", "FX6")),
        true,
    );
    seed_asset(&pool, &ws, "S-2", "camera", None, None, true);
    let s3 = seed_asset(
        &pool,
        &ws,
        "S-3",
        "camera",
        None,
        Some((cat.fx6, "Sony", "FX6")),
        true,
    );
    let file = "Name,Tag,Make,Model,Groups\n\
                S-1,S-1,Sony,PXW-Z280,\n\
                S-2,S-2,sony,fx6,\n\
                S-3,S-3,sony,fx6,Studio A\n";
    let mapping: &[(&str, &str)] = &[
        ("Name", "name"),
        ("Tag", "asset_tag"),
        ("Make", "manufacturer"),
        ("Model", "model"),
        ("Groups", "groups"),
    ];
    let job = checked(&pool, &ws, file, mapping, serde_json::json!({})).await;
    let message = |tag: &str| {
        format!(
            "{tag} is synced from Microsoft Intune, which sets its model; unmanage it on the asset page to edit it here"
        )
    };
    assert_eq!(
        errors(&job),
        vec![
            (2, Some("model".to_string()), message("S-1")),
            (3, Some("model".to_string()), message("S-2")),
        ]
    );
    assert_eq!(job["summary"]["would_update"], 1);

    commit(&pool, &ws, &id_of(&job)).await;
    assert_eq!(stored(&pool, "S-1").expect("S-1").model_id, Some(cat.fx6));
    assert_eq!(stored(&pool, "S-2").expect("S-2").model_id, None);
    assert_eq!(group_names(&pool, s3), vec!["Studio A"]);
}

/// Match values guesses a manufacturer spelled close to one the catalog
/// has, and answers models under the manufacturer the row then has.
#[actix_web::test]
async fn a_close_manufacturer_is_guessed_and_models_follow_it() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ws, cat) = workspace(&pool);
    let file = "Name,Type,Tag,Make,Model,Mount\n\
                A,,G-1,Sony Corp.,FX6,\n";
    let job = checked(&pool, &ws, file, CAMERA_MAPPING, serde_json::json!({})).await;
    assert_eq!(errors(&job), vec![], "{job}");
    let id = id_of(&job);
    let view = get_values(&pool, &ws, &id).await;
    let corp = value(column(&view, "manufacturer", ""), "Sony Corp.");
    assert_eq!(corp["match"], "guessed");
    assert_eq!(
        corp["reason"],
        serde_json::json!({ "code": "close_to", "label": "Sony" })
    );
    assert_eq!(
        corp["suggested"],
        serde_json::json!({ "action": "match", "target": cat.sony.to_string() })
    );
    assert_eq!(
        value(column(&view, "model", "sony"), "FX6")["match"],
        "exact"
    );

    // Kept as text instead, the manufacturer has no models to link.
    let (status, job) = put_values(
        &pool,
        &ws,
        &id,
        serde_json::json!({ "manufacturer": { "": { "Sony Corp.": { "action": "keep_text" } } } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(
        errors(&job),
        vec![(2, Some("kind".to_string()), "kind is required".to_string())]
    );
    let view = get_values(&pool, &ws, &id).await;
    let models = column(&view, "model", "sony corp.");
    assert_eq!(value(models, "FX6")["match"], "no_match");
}

async fn preview_values(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    id: &str,
    answers: serde_json::Value,
) -> serde_json::Value {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::post()
            .uri(&format!("/api/admin/import/{id}/values/preview"))
            .set_json(serde_json::json!({ "answers": answers })),
    )
    .await;
    let view = json(&body);
    assert_eq!(status, StatusCode::OK, "{view}");
    view
}

/// The Model card recomputed for manufacturer answers not yet checked:
/// models are matched under the manufacturer each answer gives the rows,
/// can be created under a manufacturer the import creates, and need one
/// under a manufacturer kept as text. Nothing is stored.
#[actix_web::test]
async fn models_are_recomputed_for_manufacturer_answers_before_the_check() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ws, _) = workspace(&pool);
    let file = "Name,Type,Tag,Make,Model,Mount\n\
                A,camera,G-1,Sony Corp.,FX6,\n\
                B,camera,G-2,Sony Corp.,Venice 2,\n\
                C,camera,G-3,,Pocket 6K,\n";
    let job = checked(&pool, &ws, file, CAMERA_MAPPING, serde_json::json!({})).await;
    let id = id_of(&job);
    let view = get_values(&pool, &ws, &id).await;
    let sony = column(&view, "model", "sony");
    assert_eq!(sony["can_create"], true);
    assert_eq!(value(sony, "FX6")["match"], "exact");
    assert!(value(sony, "Venice 2").get("reason").is_none());
    let none = column(&view, "model", "");
    assert_eq!(none["can_create"], false);
    assert_eq!(
        value(none, "Pocket 6K")["reason"],
        serde_json::json!({ "code": "needs_manufacturer" })
    );

    let make = |action: &str| serde_json::json!({ "manufacturer": { "": { "sony corp.": { "action": action } } } });
    let kept = preview_values(&pool, &ws, &id, make("keep_text")).await;
    let text = column(&kept, "model", "sony corp.");
    assert_eq!(text["can_create"], false);
    assert_eq!(value(text, "FX6")["match"], "no_match");
    assert_eq!(
        value(text, "Venice 2")["reason"],
        serde_json::json!({ "code": "needs_manufacturer" })
    );
    assert!(kept["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .all(|c| !(c["field"] == "model" && c["context"] == "sony")));

    let created = preview_values(&pool, &ws, &id, make("create")).await;
    let new = column(&created, "model", "sony corp.");
    assert_eq!(new["can_create"], true);
    assert!(value(new, "Venice 2").get("reason").is_none());

    // A preview stores nothing.
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["answers"], serde_json::Value::Null);
}

/// A row that links an asset to another model of the same name shows the
/// change with each model's manufacturer, whether or not the asset's
/// manufacturer text changes with it.
#[actix_web::test]
async fn a_same_name_model_relink_names_both_manufacturers() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ws, cat) = workspace(&pool);
    let canon = insert_id(
        &pool,
        "SELECT id FROM manufacturers WHERE name = $1 AND workspace_id = $2",
        &ws,
        "Canon",
    );
    seed_model(&pool, &ws, canon, "FX6", "camera", serde_json::json!({}));
    seed_asset(
        &pool,
        &ws,
        "R-1",
        "camera",
        None,
        Some((cat.fx6, "Sony", "FX6")),
        false,
    );
    // Its manufacturer text already reads Canon; only the link moves.
    seed_asset(
        &pool,
        &ws,
        "R-2",
        "camera",
        None,
        Some((cat.fx6, "Canon", "FX6")),
        false,
    );
    let file = "Name,Type,Tag,Make,Model,Mount\n\
                R-1,camera,R-1,Canon,FX6,\n\
                R-2,camera,R-2,Canon,FX6,\n";
    let job = checked(&pool, &ws, file, CAMERA_MAPPING, serde_json::json!({})).await;
    assert_eq!(errors(&job), vec![], "{job}");
    assert_eq!(job["summary"]["would_update"], 2, "{job}");
    let id = id_of(&job);

    let rows = rows(&pool, &ws, &id).await;
    let changes = |tag: &str| {
        rows.iter()
            .find(|r| r["key"] == tag)
            .unwrap_or_else(|| panic!("no row {tag} in {rows:?}"))["fields_written"]
            .clone()
    };
    let r1 = changes("R-1");
    assert_eq!(
        r1["model"],
        serde_json::json!({ "before": "FX6 (Sony)", "after": "FX6 (Canon)" }),
        "{r1}"
    );
    assert_eq!(
        r1["manufacturer"],
        serde_json::json!({ "before": "Sony", "after": "Canon" }),
        "{r1}"
    );
    let r2 = changes("R-2");
    assert_eq!(
        r2["model"],
        serde_json::json!({ "before": "FX6 (Sony)", "after": "FX6 (Canon)" }),
        "{r2}"
    );
    assert!(r2.get("manufacturer").is_none(), "{r2}");

    // The commit makes the same plan and links both to Canon's FX6.
    commit(&pool, &ws, &id).await;
    let canon_fx6 = stored(&pool, "R-1").expect("R-1").model_id;
    assert_ne!(canon_fx6, Some(cat.fx6));
    assert!(canon_fx6.is_some());
    let r2 = stored(&pool, "R-2").expect("R-2");
    assert_eq!(r2.model_id, canon_fx6);
    assert_eq!(r2.manufacturer.as_deref(), Some("Canon"));
}
