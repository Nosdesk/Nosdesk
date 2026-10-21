//! An imported workspace's files open in their new home. An attachment
//! keeps its key and url; a notes image moves to its ticket's new id, and
//! the document's image `src` with it; an asset photo and its thumbnail move
//! to the asset's new id with their `asset_media` urls; the branding logo
//! lands under the target's own address. A file the upload validator
//! refuses, or one a row names that the archive doesn't carry, stays out
//! and is reported while the rest import, and a second restore puts
//! nothing. People named only by references without a foreign key travel
//! with the export and land mapped.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Binary, Integer, Nullable, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::handlers::portal::PortalContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::services::docs_markdown::encode::{decode, encode};
use backend::services::docs_markdown::ir::{Block, Document, Image, Inline};
use backend::services::workspace_archive::apply::Applied;
use backend::services::workspace_archive::files::{self, SkipReason};
use backend::services::workspace_import::{read_archive, ArchiveContents};
use backend::sync::actor::ActorContext;
use backend::utils::storage::WorkspaceScopedStorage;

use crate::common::{insert_plain_user, seed_two_workspaces, test_storage, TestDb, TestPool};
use crate::workspace_export_hygiene::call_admin_route;
use crate::workspace_import_apply::{hosted_target, import_into, provision_hosted, Provisioned};

const PASSWORD: &str = "files-password";

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";
const WEBP: &[u8] = b"RIFF\x1a\0\0\0WEBPVP8 \x0e\0\0\0";
const PDF: &[u8] = b"%PDF-1.7\n%%EOF\n";
const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>";

/// The references without a foreign key, each naming its own person.
const NO_FK: &[(&str, &str)] = &[
    ("article_content_revisions", "contributed_by[1]"),
    ("assignment_log", "previous_assignee_uuid"),
    ("assignment_log", "new_assignee_uuid"),
    ("outbound_emails", "skip_if_seen_by"),
    ("ticket_merges", "merged_by_user_uuid"),
];

/// The source workspace's files, by their workspace-relative key, and the
/// people only the references without a foreign key name.
struct Source {
    archive: Vec<u8>,
    ticket: i32,
    attachment: String,
    note: String,
    asset: i32,
    photo: String,
    thumb: String,
    people: Vec<Uuid>,
}

/// A random high workspace id next: every sandbox database numbers
/// workspaces from 1 and the storage root is shared by the whole binary.
fn fresh_workspace_ids(conn: &mut PgConnection) {
    let id = 1_000_000 + (Uuid::new_v4().as_u128() % 1_000_000_000) as i64;
    diesel::sql_query("SELECT setval(pg_get_serial_sequence('public.workspaces', 'id'), $1)")
        .bind::<BigInt, _>(id)
        .execute(conn)
        .expect("workspace id");
}

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

fn insert_id(conn: &mut PgConnection, sql: &str, workspace_id: i32) -> i32 {
    diesel::sql_query(sql)
        .bind::<Integer, _>(workspace_id)
        .get_result::<Id>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .id
}

/// A notes document showing one image.
fn notes_with_image(src: &str) -> Vec<u8> {
    encode(&Document {
        blocks: vec![Block::Paragraph(vec![Inline::Image(Image {
            src: src.to_string(),
            alt: Some("screenshot".into()),
            ..Image::default()
        })])],
    })
}

async fn source(pool: &TestPool) -> Source {
    let mut conn = pool.get().expect("conn");
    fresh_workspace_ids(&mut conn);
    let seed = seed_two_workspaces(&mut conn).a;
    let ws = seed.workspace_id;
    let tag = Uuid::now_v7().simple().to_string();

    let new_ticket = |conn: &mut PgConnection, title: &str| {
        insert_id(
            conn,
            &format!(
                "INSERT INTO tickets (workspace_id, title, workflow_state_id) \
                 VALUES ($1, '{title}', (SELECT id FROM workflow_states WHERE workspace_id = $1 \
                                         ORDER BY id LIMIT 1)) RETURNING id"
            ),
            ws,
        )
    };
    let ticket = new_ticket(&mut conn, "Projector flickers");
    let merged = new_ticket(&mut conn, "Projector flickers again");

    let attachment = format!("tickets/{ticket}/{tag}_invoice.pdf");
    let note = format!("tickets/{ticket}/notes/{tag}_shot.png");
    let comment = diesel::sql_query(
        "INSERT INTO comments (workspace_id, ticket_id, user_uuid, content) \
         VALUES ($1, $2, $3, '<p>Invoice attached</p>') RETURNING id",
    )
    .bind::<Integer, _>(ws)
    .bind::<Integer, _>(ticket)
    .bind::<SqlUuid, _>(seed.admin_uuid)
    .get_result::<Id>(&mut conn)
    .expect("comment")
    .id;
    diesel::sql_query(
        "INSERT INTO attachments (workspace_id, url, name, comment_id, uploaded_by) \
         VALUES ($1, $2, 'invoice.pdf', $3, $4)",
    )
    .bind::<Integer, _>(ws)
    .bind::<Text, _>(format!("/uploads/{attachment}"))
    .bind::<Integer, _>(comment)
    .bind::<SqlUuid, _>(seed.admin_uuid)
    .execute(&mut conn)
    .expect("attachment");

    // The ticket's notes show the image; a revision of them names a
    // contributor, and other history names people the same way.
    let people: Vec<Uuid> = NO_FK
        .iter()
        .map(|(table, _)| insert_plain_user(&mut conn, &format!("Named by {table}")))
        .collect();
    let doc = notes_with_image(&format!("/uploads/{note}"));
    let article = diesel::sql_query(
        "INSERT INTO article_contents (workspace_id, ticket_id, yjs_document, yjs_state_vector) \
         VALUES ($1, $2, $3, '\\x00') RETURNING id",
    )
    .bind::<Integer, _>(ws)
    .bind::<Integer, _>(ticket)
    .bind::<Binary, _>(&doc)
    .get_result::<Id>(&mut conn)
    .expect("notes")
    .id;
    diesel::sql_query(
        "INSERT INTO article_content_revisions (workspace_id, article_content_id, \
           revision_number, yjs_state_vector, yjs_document_content, contributed_by) \
         VALUES ($1, $2, 1, '\\x00', $3, ARRAY[$4]::uuid[])",
    )
    .bind::<Integer, _>(ws)
    .bind::<Integer, _>(article)
    .bind::<Binary, _>(&doc)
    .bind::<SqlUuid, _>(people[0])
    .execute(&mut conn)
    .expect("revision");
    diesel::sql_query(
        "INSERT INTO assignment_log (workspace_id, ticket_id, trigger_type, method, \
           previous_assignee_uuid, new_assignee_uuid) \
         VALUES ($1, $2, 'manual', 'direct_user', $3, $4)",
    )
    .bind::<Integer, _>(ws)
    .bind::<Integer, _>(ticket)
    .bind::<SqlUuid, _>(people[1])
    .bind::<SqlUuid, _>(people[2])
    .execute(&mut conn)
    .expect("assignment log");
    diesel::sql_query(
        "INSERT INTO outbound_emails (workspace_id, ticket_id, recipient, subject, body_text, \
           message_id, skip_if_seen_by) \
         VALUES ($1, $2, 'someone@example.com', 'Re: Projector', 'Hello', $3, $4)",
    )
    .bind::<Integer, _>(ws)
    .bind::<Integer, _>(ticket)
    .bind::<Text, _>(format!("<{tag}@example.com>"))
    .bind::<SqlUuid, _>(people[3])
    .execute(&mut conn)
    .expect("outbound email");
    diesel::sql_query(
        "INSERT INTO ticket_merges (workspace_id, ticket_id, merged_into_ticket_id, merged_at, \
           merged_by_user_uuid) VALUES ($1, $2, $3, now(), $4)",
    )
    .bind::<Integer, _>(ws)
    .bind::<Integer, _>(merged)
    .bind::<Integer, _>(ticket)
    .bind::<SqlUuid, _>(people[4])
    .execute(&mut conn)
    .expect("ticket merge");

    let asset = insert_id(
        &mut conn,
        "INSERT INTO assets (workspace_id, name) VALUES ($1, 'Projector') RETURNING id",
        ws,
    );
    let photo = format!("assets/{asset}/media/{tag}_photo.png");
    let thumb = format!("assets/{asset}/media/thumb/{tag}.webp");
    diesel::sql_query(
        "INSERT INTO asset_media (workspace_id, asset_id, url, thumbnail_url, name, kind) \
         VALUES ($1, $2, $3, $4, 'photo.png', 'photo')",
    )
    .bind::<Integer, _>(ws)
    .bind::<Integer, _>(asset)
    .bind::<Text, _>(format!("/api/files/{photo}"))
    .bind::<Text, _>(format!("/api/files/{thumb}"))
    .execute(&mut conn)
    .expect("asset media");

    diesel::sql_query(
        "INSERT INTO site_settings (workspace_id, logo_url) VALUES ($1, $2) \
         ON CONFLICT (workspace_id) DO UPDATE SET logo_url = EXCLUDED.logo_url",
    )
    .bind::<Integer, _>(ws)
    .bind::<Text, _>(format!(
        "/uploads/branding/{}/logo.png?v=5",
        seed.workspace_uuid
    ))
    .execute(&mut conn)
    .expect("logo");
    drop(conn);

    let storage = WorkspaceScopedStorage::arc(test_storage(), ws);
    for (key, bytes) in [
        (attachment.as_str(), PDF),
        (note.as_str(), PNG),
        (photo.as_str(), PNG),
        (thumb.as_str(), WEBP),
        ("branding/logo.png", PNG),
    ] {
        storage
            .put_file(bytes, key, "application/octet-stream")
            .await
            .expect("source file");
    }

    let (status, body) = call_admin_route(
        pool,
        &seed,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/workspaces/{ws}/export"))
            .set_json(serde_json::json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    // A Message-ID is unique on the instance, and the target here shares the
    // source's database.
    diesel::sql_query("DELETE FROM outbound_emails WHERE workspace_id = $1")
        .bind::<Integer, _>(ws)
        .execute(&mut pool.get().expect("conn"))
        .expect("source mail");
    Source {
        archive: body.to_vec(),
        ticket,
        attachment,
        note,
        asset,
        photo,
        thumb,
        people,
    }
}

fn import(pool: &TestPool, contents: &ArchiveContents) -> (Provisioned, Applied) {
    let target = provision_hosted(pool);
    let applied = import_into(pool, contents, &hosted_target(&target)).expect("imports");
    (target, applied)
}

fn one_int(pool: &TestPool, sql: &str, workspace_id: i32) -> i32 {
    insert_id(&mut pool.get().expect("conn"), sql, workspace_id)
}

fn text(pool: &TestPool, sql: &str, workspace_id: i32) -> Option<String> {
    #[derive(QueryableByName)]
    struct Value {
        #[diesel(sql_type = Nullable<Text>)]
        value: Option<String>,
    }
    diesel::sql_query(sql)
        .bind::<Integer, _>(workspace_id)
        .get_result::<Value>(&mut pool.get().expect("conn"))
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .value
}

/// The file routes as `user`, a member of `workspace`, as their
/// middlewares would set them up.
macro_rules! app_as {
    ($pool:expr, $workspace:expr, $workspace_uuid:expr, $user:expr) => {{
        let user: Uuid = $user;
        let workspace_id: i32 = $workspace;
        let workspace_uuid: Uuid = $workspace_uuid;
        let now = chrono::Utc::now().timestamp();
        let claims = Claims {
            sub: user.to_string(),
            name: "Owner".to_string(),
            email: "owner@example.com".to_string(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (now + 3600) as usize,
            iat: now as usize,
        };
        let workspace = WorkspaceContext {
            workspace_id,
            workspace_uuid,
            slug: "target".to_string(),
            name: "Target".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let portal = PortalContext {
            user_uuid: user,
            workspace_id,
            workspace_uuid,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user, Some(corr)).with_workspace(workspace_id);
        http_test::init_service(
            App::new()
                .app_data(web::Data::new($pool.clone()))
                .app_data(web::Data::new(test_storage()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut().insert(portal.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .route(
                    "/api/files/tickets/{ticket_id}/notes/{filename:.*}",
                    web::get().to(backend::handlers::serve_ticket_note_image),
                )
                .route(
                    "/api/files/tickets/{filename:.*}",
                    web::get().to(backend::handlers::serve_ticket_file),
                )
                .route(
                    "/api/files/assets/{asset_id}/media/{filename:.*}",
                    web::get().to(backend::handlers::asset_media::serve_asset_media_file),
                )
                .route(
                    "/uploads/branding/{workspace_uuid}/{filename}",
                    web::get().to(backend::handlers::branding::serve_workspace_branding_file),
                ),
        )
        .await
    }};
}

/// `GET uri` through the app: its status and body.
macro_rules! get {
    ($app:expr, $uri:expr) => {{
        let resp =
            http_test::call_service($app, http_test::TestRequest::get().uri($uri).to_request())
                .await;
        let status = resp.status();
        (status, http_test::read_body(resp).await.to_vec())
    }};
}

/// The agent route for a stored file's `/uploads/` url.
fn agent_route(url: &str) -> String {
    url.replacen("/uploads/", "/api/files/", 1)
}

#[actix_web::test]
async fn imported_files_open_in_the_new_workspace() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool).await;
    let contents = read_archive(&src.archive, Some(PASSWORD)).expect("read");
    let carried: Vec<&str> = contents.files.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        carried.contains(&"files/branding/logo.png"),
        "the export carries the branding logo: {carried:?}"
    );

    let (target, applied) = import(&pool, &contents);
    assert!(
        applied.files.skipped.is_empty(),
        "{:?}",
        applied.files.skipped
    );
    let restored = files::restore(test_storage(), target.id, &contents, &applied.files).await;
    assert_eq!(
        (
            restored.restored,
            restored.present,
            restored.failed.as_deref()
        ),
        (5, 0, None),
        "{restored:?}"
    );
    assert!(restored.skipped.is_empty(), "{:?}", restored.skipped);

    // A second run puts nothing.
    let again = files::restore(test_storage(), target.id, &contents, &applied.files).await;
    assert_eq!(
        (again.restored, again.present, again.failed.as_deref()),
        (0, 5, None),
        "{again:?}"
    );

    let ticket = one_int(
        &pool,
        "SELECT id FROM tickets WHERE workspace_id = $1 AND title = 'Projector flickers'",
        target.id,
    );
    let asset = one_int(
        &pool,
        "SELECT id FROM assets WHERE workspace_id = $1",
        target.id,
    );
    assert_ne!(ticket, src.ticket);
    assert_ne!(asset, src.asset);
    let workspace_uuid = Uuid::parse_str(
        &text(
            &pool,
            "SELECT uuid::text AS value FROM workspaces WHERE id = $1",
            target.id,
        )
        .expect("uuid"),
    )
    .expect("uuid");
    let app = app_as!(pool, target.id, workspace_uuid, target.owner);

    // The attachment keeps its key and url.
    let url = text(
        &pool,
        "SELECT url AS value FROM attachments WHERE workspace_id = $1",
        target.id,
    )
    .expect("attachment url");
    assert_eq!(url, format!("/uploads/{}", src.attachment));
    let (status, body) = get!(&app, &agent_route(&url));
    assert_eq!(status, StatusCode::OK, "attachment");
    assert_eq!(body, PDF);

    // The notes image moves with its ticket, and the document's src with it.
    #[derive(QueryableByName)]
    struct Notes {
        #[diesel(sql_type = Binary)]
        yjs_document: Vec<u8>,
    }
    let notes: Notes = diesel::sql_query(
        "SELECT yjs_document FROM article_contents WHERE workspace_id = $1 AND ticket_id = $2",
    )
    .bind::<Integer, _>(target.id)
    .bind::<Integer, _>(ticket)
    .get_result(&mut pool.get().expect("conn"))
    .expect("notes");
    let json = decode::to_json(&notes.yjs_document);
    let src_attr = json["content"][0]["content"][0]["attrs"]["src"]
        .as_str()
        .expect("image src")
        .to_string();
    let moved_note = src.note.replacen(
        &format!("tickets/{}/", src.ticket),
        &format!("tickets/{ticket}/"),
        1,
    );
    assert_eq!(src_attr, format!("/uploads/{moved_note}"));
    let (status, body) = get!(&app, &agent_route(&src_attr));
    assert_eq!(status, StatusCode::OK, "notes image at {src_attr}");
    assert_eq!(body, PNG);

    // The asset photo and its thumbnail move with the asset, urls and all.
    let moved = |key: &str| {
        key.replacen(
            &format!("assets/{}/", src.asset),
            &format!("assets/{asset}/"),
            1,
        )
    };
    for (column, key, bytes) in [
        ("url", &src.photo, PNG),
        ("thumbnail_url", &src.thumb, WEBP),
    ] {
        let url = text(
            &pool,
            &format!("SELECT {column} AS value FROM asset_media WHERE workspace_id = $1"),
            target.id,
        )
        .expect("asset media url");
        assert_eq!(url, format!("/api/files/{}", moved(key)), "{column}");
        let (status, body) = get!(&app, &url);
        assert_eq!(status, StatusCode::OK, "asset media {column}");
        assert_eq!(body, bytes);
    }

    // The logo is the target's own.
    let logo = text(
        &pool,
        "SELECT logo_url AS value FROM site_settings WHERE workspace_id = $1",
        target.id,
    )
    .expect("logo");
    assert_eq!(
        logo,
        format!("/uploads/branding/{workspace_uuid}/logo.png?v=5")
    );
    let (status, body) = get!(&app, &logo);
    assert_eq!(status, StatusCode::OK, "logo");
    assert_eq!(body, PNG);

    // The people only references without a foreign key name came with the
    // export and are mapped, not cleared.
    for ((table, column), archive) in NO_FK.iter().zip(&src.people) {
        let person = applied
            .report
            .people
            .iter()
            .find(|p| p.archive_uuid == *archive)
            .unwrap_or_else(|| panic!("{table}.{column}'s person is in the export"));
        let value = text(
            &pool,
            &format!("SELECT {column}::text AS value FROM {table} WHERE workspace_id = $1"),
            target.id,
        );
        assert_eq!(
            value,
            Some(person.target_uuid.to_string()),
            "{table}.{column}"
        );
    }
}

/// A file the validator refuses and one the archive lost stay out and are
/// reported; the rows and every other file import.
#[actix_web::test]
async fn a_bad_or_missing_file_is_skipped_and_reported() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool).await;
    let mut contents = read_archive(&src.archive, Some(PASSWORD)).expect("read");
    let note = format!("files/{}", src.note);
    let thumb = format!("files/{}", src.thumb);
    for (name, bytes) in &mut contents.files {
        if *name == note {
            *bytes = SVG.to_vec();
        }
    }
    contents.files.retain(|(name, _)| *name != thumb);

    let (target, applied) = import(&pool, &contents);
    let restored = files::restore(test_storage(), target.id, &contents, &applied.files).await;
    assert_eq!(restored.failed, None);
    assert_eq!(restored.restored, 3, "{restored:?}");
    let skipped: Vec<(&str, SkipReason)> = restored
        .skipped
        .iter()
        .map(|s| (s.path.as_str(), s.reason))
        .collect();
    assert_eq!(
        skipped,
        vec![
            (src.note.as_str(), SkipReason::Refused),
            (src.thumb.as_str(), SkipReason::Missing),
        ]
    );
    let report = serde_json::to_value(&restored).expect("serialises");
    assert_eq!(report["skipped"][0]["reason"], "refused");

    assert_eq!(
        one_int(
            &pool,
            "SELECT count(*)::int AS id FROM asset_media WHERE workspace_id = $1",
            target.id
        ),
        1,
        "the rows import"
    );
    let url = text(
        &pool,
        "SELECT url AS value FROM attachments WHERE workspace_id = $1",
        target.id,
    )
    .expect("attachment url");
    let workspace_uuid = Uuid::parse_str(
        &text(
            &pool,
            "SELECT uuid::text AS value FROM workspaces WHERE id = $1",
            target.id,
        )
        .expect("uuid"),
    )
    .expect("uuid");
    let app = app_as!(pool, target.id, workspace_uuid, target.owner);
    let (status, _) = get!(&app, &agent_route(&url));
    assert_eq!(status, StatusCode::OK, "the rest still serve");
}
