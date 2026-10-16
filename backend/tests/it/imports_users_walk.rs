//! The users importer's First name and Last name fields, the Unchanged
//! diff for members, and Done's sign-in line, walked end to end as Alex
//! does it (design §13): eight rows from HR, one of them an existing
//! member who reads Unchanged, seven created, and "These 7 people can't
//! sign in yet."

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use uuid::Uuid;

use backend::services::imports::{self, csv_parser, ImportContext, ImportType};
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_context};

use crate::common::{TestDb, TestPool, TestPooledConn, WorkspaceSeed};
use crate::imports_assets::{
    call_import_route, import_workspaces, json, mapping_request, upload_request_for,
};
use crate::imports_users::{add_primary_email, grant};

const REF: &str = "test:imports_users_walk";

/// Alex's file: seven new starters and Sam, who is already a member.
const HR_FILE: &str = "First,Last,Email,Dept\n\
    Ada,Lovelace,ada@example.com,Engineering\n\
    Grace,Hopper,grace@example.com,Engineering\n\
    Sam,Lee,sam.lee@example.com,Operations\n\
    Alan,Turing,alan@example.com,Research\n\
    Katherine,Johnson,katherine@example.com,Research\n\
    Edsger,Dijkstra,edsger@example.com,Engineering\n\
    Barbara,Liskov,barbara@example.com,Engineering\n\
    Margaret,Hamilton,margaret@example.com,Operations\n";

async fn call(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    req: TestRequest,
) -> (StatusCode, serde_json::Value) {
    let (status, body) = call_import_route(pool, ws, ws.admin_uuid, req).await;
    (status, json(&body))
}

/// Sam Lee, a member of `ws` with the role the import calls `user`.
fn sam(conn: &mut TestPooledConn, ws: &WorkspaceSeed) -> Uuid {
    let sam = crate::common::insert_plain_user(conn, "Sam Lee");
    add_primary_email(conn, sam, "sam.lee@example.com");
    grant(conn, ws.workspace_id, sam, "member");
    sam
}

fn updated_at(pool: &TestPool, ws: &WorkspaceSeed, user: Uuid) -> chrono::NaiveDateTime {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        use backend::schema::users::dsl as u;
        u::users
            .find(user)
            .select(u::updated_at)
            .first::<chrono::NaiveDateTime>(c)
    })
    .expect("read updated_at")
}

fn name_of(pool: &TestPool, ws: &WorkspaceSeed, email: &str) -> Option<String> {
    let email = email.to_string();
    run_in_workspace(pool, REF, ws.workspace_id, move |c| {
        use backend::schema::{user_emails, users};
        users::table
            .inner_join(user_emails::table.on(user_emails::user_uuid.eq(users::uuid)))
            .filter(user_emails::email.eq(email))
            .select(users::name)
            .first::<String>(c)
            .optional()
    })
    .expect("read name")
}

fn invites_request(id: &str) -> TestRequest {
    TestRequest::post().uri(&format!("/api/admin/import/{id}/invites"))
}

/// Alex drops the HR file on Start with Assets still picked, switches to
/// users, and checks it with the Role constant Map pre-fills: seven new,
/// Sam unchanged (the constant never touches a member), Dept not imported.
/// Done says the seven can't sign in yet. Nothing was sent.
#[actix_web::test]
async fn alex_imports_eight_new_starters() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let sam = sam(&mut pool.get().expect("conn"), ws);
    let sam_before = updated_at(&pool, ws, sam);

    let (status, job) = call(
        &pool,
        ws,
        upload_request_for("assets", "new-starters.csv", HR_FILE.as_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let id = job["id"].as_str().expect("id").to_string();

    // Switch to users: Email matches its field, First and Last are guesses.
    let (status, job) = call(
        &pool,
        ws,
        TestRequest::put()
            .uri(&format!("/api/admin/import/{id}/type"))
            .set_json(serde_json::json!({ "type": "users" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let suggested: Vec<(u64, Option<String>, String)> = job["suggested_mapping"]
        .as_array()
        .expect("suggestions")
        .iter()
        .map(|s| {
            (
                s["index"].as_u64().unwrap_or(99),
                s["field"].as_str().map(String::from),
                s["reason"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    assert_eq!(
        suggested,
        vec![
            (0, Some("first_name".into()), "synonym".into()),
            (1, Some("last_name".into()), "synonym".into()),
            (2, Some("email".into()), "label".into()),
        ],
        "{job}"
    );

    let (status, checked) = call(
        &pool,
        ws,
        mapping_request(
            &id,
            serde_json::json!({
                "mapping": { "first_name": 0, "last_name": 1, "email": 2 },
                "options": {
                    "constants": [{ "field": "role", "value": "user", "apply_to": "new" }]
                }
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{checked}");
    let summary = &checked["summary"];
    assert_eq!(summary["row_count"], 8, "{summary}");
    assert_eq!(summary["would_create"], 7, "{summary}");
    assert_eq!(summary["would_update"], 0, "{summary}");
    assert_eq!(summary["unchanged"], 1, "{summary}");
    assert_eq!(summary["rows_with_errors"], 0, "{summary}");
    assert_eq!(summary["unmapped"][0]["index"], 3, "{summary}");

    let (status, page) = call(
        &pool,
        ws,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/rows?filter=unchanged")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["total"], 1, "{page}");
    assert_eq!(page["rows"][0]["key"], "sam.lee@example.com", "{page}");
    assert_eq!(page["rows"][0]["label"], "Sam Lee", "{page}");
    assert_eq!(page["rows"][0]["record_id"], sam.to_string(), "{page}");

    let (status, done) = call(
        &pool,
        ws,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["status"], "done", "{done}");
    assert_eq!(done["records_committed"], 7, "{done}");
    // The sign-in line: nothing was sent, and the tests run without email.
    assert_eq!(
        done["sign_in"],
        serde_json::json!({
            "pending": 7,
            "invited": 0,
            "method": "invite",
            "email_ready": false,
        }),
        "{done}"
    );

    // The first and last names joined into each new person's name; Sam
    // is untouched.
    assert_eq!(
        name_of(&pool, ws, "ada@example.com").as_deref(),
        Some("Ada Lovelace")
    );
    assert_eq!(
        name_of(&pool, ws, "margaret@example.com").as_deref(),
        Some("Margaret Hamilton")
    );
    assert_eq!(updated_at(&pool, ws, sam), sam_before);

    let (status, page) = call(
        &pool,
        ws,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/rows?filter=unchanged")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["phase"], "committed", "{page}");
    assert_eq!(page["total"], 1, "{page}");

    // A reload reads the same line.
    let (status, job) = call(
        &pool,
        ws,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["sign_in"]["pending"], 7, "{job}");

    // Someone who sets a password stops counting.
    let ada = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        use backend::schema::user_emails::dsl as e;
        e::user_emails
            .filter(e::email.eq("ada@example.com"))
            .select(e::user_uuid)
            .first::<Uuid>(c)
    })
    .expect("ada");
    diesel::sql_query(
        "INSERT INTO user_auth_identities (user_uuid, provider_type, external_id, password_hash) \
         VALUES ($1, 'local', $2, 'x')",
    )
    .bind::<diesel::sql_types::Uuid, _>(ada)
    .bind::<diesel::sql_types::Text, _>(ada.to_string())
    .execute(&mut pool.get().expect("conn"))
    .expect("give ada a password");
    let (_, job) = call(
        &pool,
        ws,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
    )
    .await;
    assert_eq!(job["sign_in"]["pending"], 6, "{job}");

    // Send invites needs email; here it isn't set up.
    let (status, body) = call(&pool, ws, invites_request(&id)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_invites_email_off", "{body}");

    // Another workspace's admin can't reach it.
    let (status, _) =
        call_import_route(&pool, &seeded.b, seeded.b.admin_uuid, invites_request(&id)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Send invites refuses an asset import and a draft, and an asset import
/// has no sign-in line.
#[actix_web::test]
async fn invites_are_for_committed_users_imports() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = &import_workspaces(&pool).a;

    let assets = "name,kind,asset_tag\nCam,generic,UW-1\n";
    let (_, job) = call(
        &pool,
        ws,
        upload_request_for("assets", "gear.csv", assets.as_bytes()),
    )
    .await;
    let id = job["id"].as_str().expect("id").to_string();
    let (status, body) = call(&pool, ws, invites_request(&id)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "import_invites_users_only", "{body}");

    let (_, job) = call(
        &pool,
        ws,
        upload_request_for("users", "people.csv", HR_FILE.as_bytes()),
    )
    .await;
    let id = job["id"].as_str().expect("id").to_string();
    let (status, body) = call(&pool, ws, invites_request(&id)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_not_committed", "{body}");
    assert!(job.get("sign_in").is_none(), "{job}");
}

/// Map takes First name and Last name in place of Name. With neither,
/// Map takes the file and every new person is a row error fixed on Map
/// columns: a name is needed to create a person, not to update a member.
#[actix_web::test]
async fn first_and_last_names_cover_the_name() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = &import_workspaces(&pool).a;

    let (_, job) = call(
        &pool,
        ws,
        upload_request_for("users", "people.csv", HR_FILE.as_bytes()),
    )
    .await;
    let id = job["id"].as_str().expect("id").to_string();
    let (status, body) = call(
        &pool,
        ws,
        mapping_request(
            &id,
            serde_json::json!({
                "mapping": { "email": 2 },
                "options": {
                    "constants": [{ "field": "role", "value": "user", "apply_to": "new" }]
                }
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["summary"]["would_create"], 0, "{body}");
    assert_eq!(body["summary"]["rows_with_errors"], 8, "{body}");
    let groups = body["summary"]["error_groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 1, "{body}");
    assert_eq!(groups[0]["column"], "name");
    assert_eq!(groups[0]["message"], "name is required");
    assert_eq!(groups[0]["count"], 8);
    assert_eq!(groups[0]["fix"], "map");
    let (status, body) = call(
        &pool,
        ws,
        mapping_request(
            &id,
            serde_json::json!({
                "mapping": { "email": 2, "last_name": 1 },
                "options": {
                    "constants": [{ "field": "role", "value": "user", "apply_to": "new" }]
                }
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["summary"]["would_create"], 8, "{body}");
}

fn role_of(pool: &TestPool, ws: &WorkspaceSeed, user: Uuid) -> String {
    let workspace = ws.workspace_id;
    run_in_workspace(pool, REF, workspace, move |c| {
        use backend::schema::workspace_members::dsl as m;
        m::workspace_members
            .filter(m::workspace_id.eq(workspace))
            .filter(m::user_uuid.eq(user))
            .select(m::role)
            .first::<String>(c)
    })
    .expect("read role")
}

/// A file with the email and one field to change updates the member: Map
/// takes it with no Name or Role column, and the member keeps their name.
#[actix_web::test]
async fn an_email_and_one_field_update_a_member() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = &import_workspaces(&pool).a;
    let sam = sam(&mut pool.get().expect("conn"), ws);

    let file = "Email,Role\nsam.lee@example.com,technician\n";
    let (_, job) = call(
        &pool,
        ws,
        upload_request_for("users", "roles.csv", file.as_bytes()),
    )
    .await;
    let id = job["id"].as_str().expect("id").to_string();
    let (status, body) = call(
        &pool,
        ws,
        mapping_request(
            &id,
            serde_json::json!({ "mapping": { "email": 0, "role": 1 } }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let summary = &body["summary"];
    assert_eq!(summary["would_update"], 1, "{summary}");
    assert_eq!(summary["rows_with_errors"], 0, "{summary}");
    assert_eq!(summary["changes"][0]["field"], "role", "{summary}");

    let (status, body) = call(
        &pool,
        ws,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, page) = call(
        &pool,
        ws,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/rows")),
    )
    .await;
    let row = &page["rows"][0];
    assert_eq!(row["outcome"], "update", "{page}");
    assert_eq!(row["label"], "Sam Lee", "{page}");
    assert_eq!(role_of(&pool, ws, sam), "agent");
    assert_eq!(
        name_of(&pool, ws, "sam.lee@example.com").as_deref(),
        Some("Sam Lee")
    );
}

fn parse(content: &str) -> csv_parser::ParsedCsv {
    csv_parser::parse_bytes(content.as_bytes()).expect("parse")
}

fn dry_run(
    conn: &mut TestPooledConn,
    ws: &WorkspaceSeed,
    actor: Uuid,
    parsed: &csv_parser::ParsedCsv,
) -> (imports::ImportSummary, Vec<imports::RowResult>) {
    let pin = ActorContext::user(actor, None).with_workspace(ws.workspace_id);
    let ctx = ImportContext {
        actor: Some(actor),
        job_id: Uuid::new_v4(),
        options: Default::default(),
    };
    with_actor_context::<_, diesel::result::Error>(conn, &pin, |c| {
        imports::check(c, ImportType::Users, parsed, &ctx)
    })
    .expect("dry-run")
}

/// On a member's row: a blank name or role leaves it as it is; the Name
/// cell wins over First and Last where it has a value, which fill in where
/// it is blank; a change reads Updated with its value before and after.
#[test]
fn a_members_row_changes_only_what_it_gives() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::seed_two_workspaces(&mut conn).a;
    sam(&mut conn, &ws);

    let (summary, rows) = dry_run(
        &mut conn,
        &ws,
        ws.admin_uuid,
        &parse(
            "email,name,first_name,last_name,role,pronouns\n\
             sam.lee@example.com,,,,,\n\
             sam.lee2@example.com,Display Name,Ignored,Parts,user,\n\
             sam.lee3@example.com,,Joined,Name,user,\n\
             sam.lee4@example.com,,,Mononym,user,\n",
        ),
    );
    assert_eq!(summary.unchanged, 1, "{summary:?}");
    assert_eq!(summary.would_create, 3, "{summary:?}");
    assert_eq!(rows[0].outcome, imports::RowOutcome::Unchanged);
    let labels: Vec<Option<&str>> = rows.iter().map(|r| r.label.as_deref()).collect();
    assert_eq!(
        labels,
        vec![
            Some("Sam Lee"),
            Some("Display Name"),
            Some("Joined Name"),
            Some("Mononym")
        ]
    );

    // A role the member doesn't have yet is an update, before and after.
    let (summary, rows) = dry_run(
        &mut conn,
        &ws,
        ws.admin_uuid,
        &parse("email,name,role,pronouns\nsam.lee@example.com,Sam Lee,technician,\n"),
    );
    assert_eq!(summary.would_update, 1, "{summary:?}");
    let role = rows[0].fields_written.get("role").expect("role change");
    assert_eq!(role.before.as_deref(), Some("user"));
    assert_eq!(role.after.as_deref(), Some("technician"));
    assert_eq!(summary.changes[0].field, "role");

    // A new person still needs a name and a role.
    let (summary, _) = dry_run(
        &mut conn,
        &ws,
        ws.admin_uuid,
        &parse("email,name,role,pronouns\nnew@example.com,,,\n"),
    );
    let errors: Vec<(Option<&str>, &str)> = summary
        .errors
        .iter()
        .map(|e| (e.column.as_deref(), e.message.as_str()))
        .collect();
    assert_eq!(
        errors,
        vec![
            (Some("name"), "name is required"),
            (Some("role"), "role is required"),
        ]
    );
}
