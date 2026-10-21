//! Round trip B: a self-hosted workspace is exported, imported into an
//! untouched workspace on the same server, and exported again.
//!
//! The source is rich: tickets with comments, an attachment, a mention and
//! a ticket reference, notes with a ticket link, a mention and an image, a
//! document with an image and a table, assets with a photo, a loan and a
//! disposal, a rule, a view, a saved reply, branding, people in each role
//! (some with avatars), mail in and out, and a CSV import it ran. The
//! import is checked by content, never by id: rows per table, references
//! landing on the new rows, documents naming the new ids, files at their new
//! keys, settings carried with sealed values dropped and flagged,
//! integrations off, people mapped or created. The second archive then
//! matches the first, ids aside. The same archive imports a second time on
//! the same server.
//!
//! Damaged archives (a changed, missing or unlisted table, a truncated
//! file) are refused before anything is read into the database.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};
use std::sync::LazyLock;

use actix_web::http::StatusCode;
use actix_web::test as http_test;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Binary, Integer, Nullable, Text, Uuid as SqlUuid};
use regex::Regex;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::middleware::workspace_context::DeploymentMode;
use backend::models::NewWorkspace;
use backend::repository::workspaces::{add_membership, create_workspace, SeatWriteAuthority};
use backend::services::backup::BackupError;
use backend::services::docs_markdown::encode::{decode, encode};
use backend::services::docs_markdown::ir::{Block, Document, Image, Inline, Table, TableCell};
use backend::services::seed::seed_workspace_defaults;
use backend::services::workspace_archive::apply::{self, Applied};
use backend::services::workspace_archive::files::{self, FilesRestored};
use backend::services::workspace_archive::plan::{ImportTarget, Outcome, PlanOptions, Runner};
use backend::services::workspace_import::{read_archive, ArchiveContents};
use backend::sync::actor::ActorContext;
use backend::sync::session::{with_actor_bypass_context, with_actor_context};
use backend::utils::storage::WorkspaceScopedStorage;

use crate::common::{insert_plain_user, test_storage, TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{call_import_route, import_workspaces, json as body_json};
use crate::workspace_export_hygiene::call_admin_route;

const REF: &str = "test:portability_round_trip";
const PASSWORD: &str = "round-trip-password";

const PDF: &[u8] = b"%PDF-1.7\n%%EOF\n";
const WEBP: &[u8] = b"RIFF\x1a\0\0\0WEBPVP8 \x0e\0\0\0";

const CSV_HEADER: &str =
    "name,kind,asset_tag,serial_number,manufacturer,model,location,notes,quantity,unit,low_stock_threshold";

/// A small real PNG (the avatar thumbnail decodes it).
fn png(shade: u8) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(8, 8, image::Rgb([shade, 64, 128]));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("encode png");
    out
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

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct Txt {
    #[diesel(sql_type = Nullable<Text>)]
    value: Option<String>,
}

/// Run `sql` binding `$1` to `workspace_id`; the first row's `id`.
fn id_in(conn: &mut PgConnection, sql: &str, workspace_id: i32) -> i32 {
    diesel::sql_query(sql)
        .bind::<Integer, _>(workspace_id)
        .get_result::<Id>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .id
}

fn exec(conn: &mut PgConnection, sql: &str, workspace_id: i32) {
    diesel::sql_query(sql)
        .bind::<Integer, _>(workspace_id)
        .execute(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

fn text(pool: &TestPool, sql: &str, workspace_id: i32) -> Option<String> {
    diesel::sql_query(sql)
        .bind::<Integer, _>(workspace_id)
        .get_result::<Txt>(&mut pool.get().expect("conn"))
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .value
}

fn count(pool: &TestPool, sql: &str, workspace_id: i32) -> i64 {
    diesel::sql_query(sql)
        .bind::<Integer, _>(workspace_id)
        .get_result::<Count>(&mut pool.get().expect("conn"))
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .n
}

fn bytes_of(pool: &TestPool, sql: &str, workspace_id: i32) -> Vec<u8> {
    #[derive(QueryableByName)]
    struct Bytes {
        #[diesel(sql_type = Binary)]
        value: Vec<u8>,
    }
    diesel::sql_query(sql)
        .bind::<Integer, _>(workspace_id)
        .get_result::<Bytes>(&mut pool.get().expect("conn"))
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .value
}

/// A person on the source: their uuid, and the avatar they hold there.
#[derive(Clone)]
struct Person {
    uuid: Uuid,
    name: String,
    avatar: Option<(String, Vec<u8>)>,
}

/// The source and what it was made with.
struct Source {
    seed: WorkspaceSeed,
    archive: Vec<u8>,
    admin: Person,
    ana: Person,
    rex: Person,
    ticket_numbers: (i32, i32),
    inbound_message: String,
    sent_message: String,
    attachment: String,
    note: String,
    diagram: String,
    photo: String,
    thumb: String,
}

/// A notes document: a ticket link, a mention and an image.
fn notes(ticket: i32, number: i32, ana: &Person, image: &str) -> Vec<u8> {
    encode(&Document {
        blocks: vec![Block::Paragraph(vec![
            Inline::text("Same as "),
            Inline::TicketLink {
                ticket_id: ticket.to_string(),
                href: format!("/tickets/{number}"),
            },
            Inline::text(", ask "),
            Inline::Mention {
                uuid: ana.uuid.to_string(),
                name: ana.name.clone(),
                avatar_url: ana.avatar.as_ref().map(|(url, _)| url.clone()),
            },
            Inline::Image(Image {
                src: image.to_string(),
                alt: Some("screenshot".into()),
                ..Image::default()
            }),
        ])],
    })
}

/// A document with an image and a table.
fn guide(image: &str) -> Vec<u8> {
    let cell = |header: bool, text: &str| TableCell::new(header, vec![Inline::text(text)]);
    encode(&Document {
        blocks: vec![
            Block::Paragraph(vec![Inline::Image(Image {
                src: image.to_string(),
                alt: Some("wiring".into()),
                ..Image::default()
            })]),
            Block::Table(Table {
                rows: vec![
                    vec![cell(true, "Input"), cell(true, "Cable")],
                    vec![cell(false, "HDMI 1"), cell(false, "Long")],
                ],
            }),
        ],
    })
}

/// A person with a primary email and a role in `workspace_id`.
fn add_person(
    conn: &mut crate::common::TestPooledConn,
    workspace_id: i32,
    name: &str,
    email: Option<&str>,
    role: &str,
) -> Uuid {
    let uuid = insert_plain_user(conn, name);
    if let Some(email) = email {
        diesel::sql_query(
            "INSERT INTO user_emails (user_uuid, email, email_type, is_primary, is_verified) \
             VALUES ($1, $2, 'work', true, true)",
        )
        .bind::<SqlUuid, _>(uuid)
        .bind::<Text, _>(email)
        .execute(conn)
        .expect("email");
    }
    let actor = ActorContext::system(REF).with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        add_membership(c, workspace_id, uuid, role, SeatWriteAuthority::Product)
    })
    .expect("membership");
    uuid
}

/// Give `uuid` an uploaded avatar.
async fn give_avatar(pool: &TestPool, uuid: Uuid, shade: u8) -> (String, Vec<u8>) {
    let bytes = png(shade);
    let key = format!("users/avatars/{uuid}_avatar.png");
    test_storage()
        .put_file(&bytes, &key, "image/png")
        .await
        .expect("avatar");
    let url = format!("/uploads/{key}");
    diesel::sql_query("UPDATE users SET avatar_url = $1 WHERE uuid = $2")
        .bind::<Text, _>(&url)
        .bind::<SqlUuid, _>(uuid)
        .execute(&mut pool.get().expect("conn"))
        .expect("avatar url");
    (url, bytes)
}

async fn export(pool: &TestPool, seed: &WorkspaceSeed, workspace_id: i32) -> Vec<u8> {
    let (status, body) = call_admin_route(
        pool,
        seed,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/workspaces/{workspace_id}/export"))
            .set_json(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    body.to_vec()
}

async fn source(pool: &TestPool) -> Source {
    fresh_workspace_ids(&mut pool.get().expect("conn"));
    let seed = import_workspaces(pool).a;
    let ws = seed.workspace_id;
    let tag = Uuid::now_v7().simple().to_string();

    // A CSV import the source ran: assets it made, and its history.
    let (status, body) = {
        let (status, body) = call_import_route(
            pool,
            &seed,
            seed.admin_uuid,
            crate::imports_assets::upload_request(
                "assets.csv",
                format!("{CSV_HEADER}\nCSV Laptop,generic,RT-{tag},,,,,,,,\n").as_bytes(),
            ),
        )
        .await;
        let job = body_json(&body);
        assert_eq!(status, StatusCode::OK, "{job}");
        let id = job["id"].as_str().expect("job id").to_string();
        let (status, body) = call_import_route(
            pool,
            &seed,
            seed.admin_uuid,
            crate::imports_assets::mapping_request(&id, crate::imports_assets::template_mapping()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", body_json(&body));
        call_import_route(
            pool,
            &seed,
            seed.admin_uuid,
            http_test::TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
        )
        .await
    };
    assert_eq!(status, StatusCode::OK, "{}", body_json(&body));

    let mut conn = pool.get().expect("conn");
    // People: the fixture's admin and member (no email: created on import),
    // an agent and a requester with emails (mapped to their accounts).
    let ana_uuid = add_person(
        &mut conn,
        ws,
        "Ana Agent",
        Some(&format!("ana-{tag}@example.com")),
        "agent",
    );
    let rex_uuid = add_person(
        &mut conn,
        ws,
        "Rex Requester",
        Some(&format!("rex-{tag}@example.com")),
        "member",
    );
    drop(conn);
    let admin = Person {
        uuid: seed.admin_uuid,
        name: "a Admin".into(),
        avatar: Some(give_avatar(pool, seed.admin_uuid, 10).await),
    };
    let ana = Person {
        uuid: ana_uuid,
        name: "Ana Agent".into(),
        avatar: Some(give_avatar(pool, ana_uuid, 200).await),
    };
    let rex = Person {
        uuid: rex_uuid,
        name: "Rex Requester".into(),
        avatar: None,
    };
    let mut conn = pool.get().expect("conn");

    // Tickets.
    let state = "(SELECT id FROM workflow_states WHERE workspace_id = $1 ORDER BY id LIMIT 1)";
    let t1 = id_in(
        &mut conn,
        &format!(
            "INSERT INTO tickets (workspace_id, title, workflow_state_id, requester_uuid, \
               assignee_uuid, created_by, guest_lookup_token) \
             VALUES ($1, 'Projector flickers', {state}, '{rex_uuid}', '{ana_uuid}', \
               '{}', gen_random_uuid()) RETURNING id",
            seed.admin_uuid
        ),
        ws,
    );
    let t2 = id_in(
        &mut conn,
        &format!(
            "INSERT INTO tickets (workspace_id, title, workflow_state_id, requester_uuid) \
             VALUES ($1, 'Projector cable', {state}, '{rex_uuid}') RETURNING id"
        ),
        ws,
    );
    let number = |conn: &mut PgConnection, t: i32| {
        id_in(
            conn,
            &format!("SELECT number AS id FROM tickets WHERE workspace_id = $1 AND id = {t}"),
            ws,
        )
    };
    let numbers = (number(&mut conn, t1), number(&mut conn, t2));

    // A comment linking the other ticket and mentioning Ana, with an
    // attachment, a reference row and a link between the tickets.
    let comment = id_in(
        &mut conn,
        &format!(
            "INSERT INTO comments (workspace_id, ticket_id, user_uuid, content) VALUES ($1, {t1}, \
               '{ana_uuid}', '<p>Same as <span data-ticket-link data-ticket-id=\"{t2}\" \
               data-href=\"/tickets/{n2}\">#{n2}</span>, ask <span data-mention=\"true\" \
               data-uuid=\"{ana_uuid}\" data-name=\"Ana Agent\">@Ana Agent</span></p>') \
             RETURNING id",
            n2 = numbers.1
        ),
        ws,
    );
    let attachment = format!("tickets/{t1}/{tag}_invoice.pdf");
    exec(
        &mut conn,
        &format!(
            "INSERT INTO attachments (workspace_id, url, name, comment_id, uploaded_by) \
             VALUES ($1, '/uploads/{attachment}', 'invoice.pdf', {comment}, '{ana_uuid}')"
        ),
        ws,
    );
    exec(
        &mut conn,
        &format!(
            "INSERT INTO comment_ticket_references (workspace_id, comment_id, referenced_ticket_id) \
             VALUES ($1, {comment}, {t2})"
        ),
        ws,
    );
    exec(
        &mut conn,
        &format!(
            "INSERT INTO linked_tickets (workspace_id, ticket_id, linked_ticket_id, relation_type, \
               created_by) VALUES ($1, {t1}, {t2}, 'related', '{ana_uuid}')"
        ),
        ws,
    );
    let vip = id_in(
        &mut conn,
        "INSERT INTO tags (workspace_id, name) VALUES ($1, 'vip') RETURNING id",
        ws,
    );
    exec(
        &mut conn,
        &format!(
            "INSERT INTO ticket_tags (workspace_id, ticket_id, tag_id) VALUES ($1, {t1}, {vip})"
        ),
        ws,
    );

    // The ticket's notes.
    let note = format!("tickets/{t1}/notes/{tag}_shot.png");
    diesel::sql_query(
        "INSERT INTO article_contents (workspace_id, ticket_id, yjs_document, yjs_state_vector) \
         VALUES ($1, $2, $3, '\\x00')",
    )
    .bind::<Integer, _>(ws)
    .bind::<Integer, _>(t1)
    .bind::<Binary, _>(notes(t2, numbers.1, &ana, &format!("/uploads/{note}")))
    .execute(&mut conn)
    .expect("notes");

    // A document with an image and a table.
    #[derive(QueryableByName)]
    struct Page {
        #[diesel(sql_type = Integer)]
        id: i32,
        #[diesel(sql_type = Text)]
        uuid: String,
    }
    let page: Page = diesel::sql_query(
        "INSERT INTO documentation_pages (workspace_id, title, slug, created_by, last_edited_by) \
         VALUES ($1, 'Projector guide', $2, $3, $3) RETURNING id, uuid::text AS uuid",
    )
    .bind::<Integer, _>(ws)
    .bind::<Text, _>(format!("projector-guide-{tag}"))
    .bind::<SqlUuid, _>(seed.admin_uuid)
    .get_result(&mut conn)
    .expect("page");
    let diagram = format!("collab/doc/{}/{tag}_diagram.png", page.uuid);
    diesel::sql_query(
        "UPDATE documentation_pages SET yjs_document = $2, yjs_state_vector = '\\x00' \
         WHERE workspace_id = $1 AND id = $3",
    )
    .bind::<Integer, _>(ws)
    .bind::<Binary, _>(guide(&format!("/api/files/{diagram}")))
    .bind::<Integer, _>(page.id)
    .execute(&mut conn)
    .expect("page document");

    // An asset with a photo, a loan and a disposal.
    let asset = id_in(
        &mut conn,
        "INSERT INTO assets (workspace_id, name) VALUES ($1, 'Projector') RETURNING id",
        ws,
    );
    let photo = format!("assets/{asset}/media/{tag}_photo.png");
    let thumb = format!("assets/{asset}/media/thumb/{tag}.webp");
    exec(
        &mut conn,
        &format!(
            "INSERT INTO asset_media (workspace_id, asset_id, url, thumbnail_url, name, kind) \
             VALUES ($1, {asset}, '/api/files/{photo}', '/api/files/{thumb}', 'photo.png', 'photo')"
        ),
        ws,
    );
    exec(
        &mut conn,
        &format!(
            "INSERT INTO asset_loans (workspace_id, asset_id, borrower_user_uuid, status_before, \
               ticket_id, actor_uuid) VALUES ($1, {asset}, '{rex_uuid}', 'in_stock', {t1}, '{ana_uuid}')"
        ),
        ws,
    );
    let old = id_in(
        &mut conn,
        "INSERT INTO assets (workspace_id, name) VALUES ($1, 'Old projector') RETURNING id",
        ws,
    );
    exec(
        &mut conn,
        &format!(
            "INSERT INTO asset_disposals (workspace_id, asset_id, sanitization_method, \
               data_bearing, notes, actor_uuid) VALUES ($1, {old}, 'destroy', false, 'Shredded', \
               '{ana_uuid}')"
        ),
        ws,
    );

    // A rule, a view and a saved reply.
    exec(
        &mut conn,
        &format!(
            "INSERT INTO rules (workspace_id, name, trigger_kind, actions, state) \
             VALUES ($1, 'Tag VIPs', 'manual', jsonb_build_array(jsonb_build_object('kind', \
               'add_tags', 'config', jsonb_build_object('tag_ids', jsonb_build_array({vip})))), \
               'live')"
        ),
        ws,
    );
    exec(
        &mut conn,
        &format!(
            "INSERT INTO saved_views (workspace_id, scope, name, shape, filter, created_by, dataset) \
             VALUES ($1, 'workspace', 'Ana''s VIPs', '{{}}', jsonb_build_object('predicate', \
               jsonb_build_object('kind', 'group', 'op', 'and', 'children', jsonb_build_array( \
                 jsonb_build_object('kind', 'leaf', 'field', 'tag_ids', 'op', 'in', 'value', \
                   jsonb_build_array({vip})), \
                 jsonb_build_object('kind', 'leaf', 'field', 'assignee_uuid', 'op', 'in', \
                   'value', jsonb_build_array('{ana_uuid}'))))), '{ana_uuid}', 'tickets')"
        ),
        ws,
    );
    exec(
        &mut conn,
        &format!(
            "INSERT INTO canned_responses (workspace_id, title, body, created_by) \
             VALUES ($1, 'Restart it', 'Have you tried turning it off and on?', '{ana_uuid}')"
        ),
        ws,
    );

    // Branding, and outbound email with a sealed password.
    exec(
        &mut conn,
        &format!(
            "INSERT INTO site_settings (workspace_id, app_name, primary_color, logo_url) \
             VALUES ($1, 'Acme Desk', '#123456', '/uploads/branding/{}/logo.png?v=3') \
             ON CONFLICT (workspace_id) DO UPDATE SET app_name = EXCLUDED.app_name, \
               primary_color = EXCLUDED.primary_color, logo_url = EXCLUDED.logo_url",
            seed.workspace_uuid
        ),
        ws,
    );
    exec(
        &mut conn,
        "INSERT INTO workspace_email_settings (workspace_id, enabled, from_name, from_email, \
           smtp_host, smtp_port, smtp_security, smtp_username, encrypted_smtp_password, \
           encrypted_kek_id) \
         VALUES ($1, true, 'Acme', 'help@acme.example', 'smtp.acme.example', 587, 'starttls', \
           'acme', '\\x01020304'::bytea, 1)",
        ws,
    );

    // Mail: an IMAP channel, a customer's message, the reply that went out
    // and one still queued.
    let channel = id_in(
        &mut conn,
        "INSERT INTO channels (workspace_id, provider, name, enabled, config) \
         VALUES ($1, 'email_imap', 'Support inbox', true, '{}') RETURNING id",
        ws,
    );
    let inbound_message = format!("<in-{tag}@customer.example>");
    let sent_message = format!("out-{tag}@help.acme.example");
    exec(
        &mut conn,
        &format!(
            "INSERT INTO channel_messages (workspace_id, channel_id, external_id, direction, \
               ticket_id, from_address) \
             VALUES ($1, {channel}, '{inbound_message}', 'inbound', {t1}, 'rex@example.com'), \
                    ($1, {channel}, '<{sent_message}>', 'outbound', {t1}, NULL)"
        ),
        ws,
    );
    exec(
        &mut conn,
        &format!(
            "INSERT INTO outbound_emails (workspace_id, channel_id, ticket_id, comment_id, recipient, \
               subject, body_text, message_id, in_reply_to, status, sent_at) \
             VALUES ($1, {channel}, {t1}, {comment}, 'rex@example.com', '[#{n1}] Projector', \
               'Same as before', '{sent_message}', '{inbound_message}', 'sent', now()), \
                    ($1, {channel}, {t1}, NULL, 'rex@example.com', '[#{n1}] Projector', \
               'Still there?', 'queued-{tag}@help.acme.example', '{inbound_message}', 'pending', NULL)",
            n1 = numbers.0
        ),
        ws,
    );
    drop(conn);

    // Files.
    let storage = WorkspaceScopedStorage::arc(test_storage(), ws);
    for (key, bytes) in [
        (attachment.as_str(), PDF.to_vec()),
        (note.as_str(), png(1)),
        (diagram.as_str(), png(2)),
        (photo.as_str(), png(3)),
        (thumb.as_str(), WEBP.to_vec()),
        ("branding/logo.png", png(4)),
    ] {
        storage
            .put_file(&bytes, key, "application/octet-stream")
            .await
            .expect("source file");
    }

    let archive = export(pool, &seed, ws).await;
    Source {
        seed,
        archive,
        admin,
        ana,
        rex,
        ticket_numbers: numbers,
        inbound_message,
        sent_message,
        attachment,
        note,
        diagram,
        photo,
        thumb,
    }
}

/// An untouched workspace on the same server, as a self-hosted owner would
/// have it: created, seeded, its owner a member.
struct Target {
    id: i32,
    owner: Uuid,
}

fn fresh_target(pool: &TestPool) -> Target {
    let mut conn = pool.get().expect("conn");
    fresh_workspace_ids(&mut conn);
    let tag = Uuid::new_v4().simple().to_string()[..8].to_string();
    let ws = create_workspace(
        &mut conn,
        &NewWorkspace {
            uuid: Uuid::now_v7(),
            slug: format!("round-trip-{tag}"),
            name: format!("Round trip {tag}"),
            seat_limit: None,
        },
    )
    .expect("create workspace");
    let owner = insert_plain_user(&mut conn, "Target Owner");
    let actor = ActorContext::system(REF).with_workspace(ws.id);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        seed_workspace_defaults(c, None)?;
        add_membership(c, ws.id, owner, "owner", SeatWriteAuthority::Product)
    })
    .expect("provision");
    Target { id: ws.id, owner }
}

/// Import `contents` into `target` as its self-hosted owner, then restore
/// the files, as the import job does.
async fn import(
    pool: &TestPool,
    contents: &ArchiveContents,
    target: &Target,
) -> (Applied, FilesRestored) {
    let held = apply::held_files(test_storage(), target.id)
        .await
        .expect("list");
    let mut conn = pool.get().expect("conn");
    let applied = with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        apply::import_into(
            c,
            contents,
            &ImportTarget {
                workspace_id: target.id,
                mode: DeploymentMode::SelfHosted,
                runner: Runner::SelfHostedOwner { user: target.owner },
            },
            PlanOptions {
                regenerate_row_uuids: true,
            },
            &held,
        )
    })
    .expect("imports");
    drop(conn);
    let restored = files::restore(test_storage(), target.id, contents, &applied.files).await;
    (applied, restored)
}

/// Every node of `kind` in a decoded document.
fn nodes<'a>(doc: &'a Value, kind: &str, out: &mut Vec<&'a Value>) {
    if doc.get("type").and_then(Value::as_str) == Some(kind) {
        out.push(doc);
    }
    if let Some(children) = doc.get("content").and_then(Value::as_array) {
        for child in children {
            nodes(child, kind, out);
        }
    }
}

fn nodes_of<'a>(doc: &'a Value, kind: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    nodes(doc, kind, &mut out);
    out
}

#[actix_web::test]
async fn a_workspace_exported_imported_and_exported_again_comes_back_the_same() {
    let db = TestDb::new();
    let pool = db.pool_with_size(6);
    let src = source(&pool).await;
    let first = read_archive(&src.archive, Some(PASSWORD)).expect("read the first archive");
    assert_eq!(first.avatars.len(), 2, "both avatars travel");

    let target = fresh_target(&pool);
    let (applied, restored) = import(&pool, &first, &target).await;
    let t = target.id;

    // Nothing refused, and no file stayed out.
    assert!(
        applied.report.refusals.is_empty(),
        "{:?}",
        applied.report.refusals
    );
    assert!(restored.failed.is_none(), "{restored:?}");
    assert!(restored.skipped.is_empty(), "{:?}", restored.skipped);

    // Rows: every carried table arrives whole, but the queued email.
    let mut short = Vec::new();
    for table in &applied.report.tables {
        let left_out: usize = table.dropped.values().sum();
        let expected = match table.table.as_str() {
            "outbound_emails" => 1,
            _ => 0,
        };
        if table.class != "never" && table.class != "excluded" && left_out != expected {
            short.push(format!("{}: {:?}", table.table, table.dropped));
        }
    }
    assert!(short.is_empty(), "rows left out: {short:?}");
    for table in &applied.report.tables {
        if table.class != "move" {
            continue;
        }
        let held = count(
            &pool,
            &format!(
                "SELECT count(*)::bigint AS n FROM {} WHERE workspace_id = $1",
                table.table
            ),
            t,
        );
        assert_eq!(
            held, table.planned as i64,
            "{} rows on the target",
            table.table
        );
    }
    assert_eq!(
        count(
            &pool,
            "SELECT count(*)::bigint AS n FROM import_jobs WHERE workspace_id = $1",
            t
        ),
        0,
        "CSV import history stays behind"
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*)::bigint AS n FROM assets WHERE workspace_id = $1 AND name = 'CSV Laptop'",
            t
        ),
        1,
        "what the CSV import made comes with the workspace"
    );

    // References land on the new rows.
    let ticket = |title: &str| {
        id_in(
            &mut pool.get().expect("conn"),
            &format!("SELECT id FROM tickets WHERE workspace_id = $1 AND title = '{title}'"),
            t,
        )
    };
    let (t1, t2) = (ticket("Projector flickers"), ticket("Projector cable"));
    let numbers = (
        id_in(
            &mut pool.get().expect("conn"),
            &format!("SELECT number AS id FROM tickets WHERE id = {t1} AND workspace_id = $1"),
            t,
        ),
        id_in(
            &mut pool.get().expect("conn"),
            &format!("SELECT number AS id FROM tickets WHERE id = {t2} AND workspace_id = $1"),
            t,
        ),
    );
    assert_eq!(numbers, src.ticket_numbers, "ticket numbers are kept");
    let comment = text(
        &pool,
        &format!(
            "SELECT content AS value FROM comments WHERE workspace_id = $1 AND ticket_id = {t1}"
        ),
        t,
    )
    .expect("comment");
    assert!(
        comment.contains(&format!("data-ticket-id=\"{t2}\"")),
        "{comment}"
    );
    assert!(
        comment.contains(&format!("data-uuid=\"{}\"", src.ana.uuid)),
        "{comment}"
    );
    assert_eq!(
        count(
            &pool,
            &format!(
                "SELECT count(*)::bigint AS n FROM comment_ticket_references r \
                 JOIN comments c ON c.id = r.comment_id \
                 WHERE r.workspace_id = $1 AND c.ticket_id = {t1} AND r.referenced_ticket_id = {t2}"
            ),
            t
        ),
        1
    );
    assert_eq!(
        count(
            &pool,
            &format!(
                "SELECT count(*)::bigint AS n FROM linked_tickets WHERE workspace_id = $1 \
                 AND ticket_id = {t1} AND linked_ticket_id = {t2}"
            ),
            t
        ),
        1
    );
    assert_eq!(
        text(
            &pool,
            "SELECT (r.actions -> 0 -> 'config' -> 'tag_ids' = jsonb_build_array(g.id) \
                   AND r.state = 'live')::text AS value \
                 FROM rules r JOIN tags g ON g.workspace_id = r.workspace_id AND g.name = 'vip' \
                 WHERE r.workspace_id = $1",
            t
        )
        .as_deref(),
        Some("true"),
        "the rule names the new tag and stays live"
    );
    let view = text(
        &pool,
        "SELECT v.filter::text AS value FROM saved_views v WHERE v.workspace_id = $1",
        t,
    )
    .expect("view");
    let vip = id_in(
        &mut pool.get().expect("conn"),
        "SELECT id FROM tags WHERE workspace_id = $1 AND name = 'vip'",
        t,
    );
    assert!(
        view.contains(&format!("[{vip}]")) && view.contains(&src.ana.uuid.to_string()),
        "{view}"
    );

    // Mail threads into the new ticket through the new channel.
    let channel = id_in(
        &mut pool.get().expect("conn"),
        "SELECT id FROM channels WHERE workspace_id = $1",
        t,
    );
    for message in [
        src.inbound_message.clone(),
        format!("<{}>", src.sent_message),
    ] {
        let found = backend::repository::channels::find_ticket_by_reference_chain(
            &mut pool.get().expect("conn"),
            channel,
            std::slice::from_ref(&message),
        )
        .expect("thread");
        assert_eq!(
            found,
            Some(t1),
            "a reply naming {message} threads into the new ticket"
        );
    }
    let sent = text(
        &pool,
        "SELECT message_id AS value FROM outbound_emails WHERE workspace_id = $1",
        t,
    )
    .expect("the sent email");
    assert_ne!(
        sent, src.sent_message,
        "the sent email has a Message-ID of its own"
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*)::bigint AS n FROM outbound_emails WHERE workspace_id = $1 \
             AND (status IN ('pending', 'sending', 'failed') OR message_id LIKE 'queued-%')",
            t
        ),
        0,
        "mail the source hadn't sent never reaches the target's queue"
    );
    assert!(sent.ends_with("@help.acme.example"), "{sent}");

    // Documents name the new rows, and their files are where they point.
    let notes_doc = decode::to_json(&bytes_of(
        &pool,
        &format!("SELECT yjs_document AS value FROM article_contents WHERE workspace_id = $1 AND ticket_id = {t1}"),
        t,
    ));
    let link = nodes_of(&notes_doc, "ticket_link");
    assert_eq!(link.len(), 1, "{notes_doc}");
    assert_eq!(
        link[0]["attrs"]["ticketId"].as_str(),
        Some(t2.to_string().as_str())
    );
    let mention = nodes_of(&notes_doc, "mention");
    assert_eq!(
        mention[0]["attrs"]["uuid"].as_str(),
        Some(src.ana.uuid.to_string().as_str())
    );
    let image = nodes_of(&notes_doc, "image");
    let note_src = image[0]["attrs"]["src"]
        .as_str()
        .expect("notes image")
        .to_string();
    let moved_note = src.note.replacen(
        &format!("tickets/{}/", src.seed_ticket_one()),
        &format!("tickets/{t1}/"),
        1,
    );
    assert_eq!(note_src, format!("/uploads/{moved_note}"));

    let guide_doc = decode::to_json(&bytes_of(
        &pool,
        "SELECT yjs_document AS value FROM documentation_pages WHERE workspace_id = $1 AND title = 'Projector guide'",
        t,
    ));
    assert_eq!(nodes_of(&guide_doc, "table").len(), 1, "{guide_doc}");
    let page_uuid = text(
        &pool,
        "SELECT uuid::text AS value FROM documentation_pages WHERE workspace_id = $1 AND title = 'Projector guide'",
        t,
    )
    .expect("page uuid");
    let diagram_src = nodes_of(&guide_doc, "image")[0]["attrs"]["src"]
        .as_str()
        .expect("diagram")
        .to_string();
    assert_eq!(
        diagram_src,
        format!(
            "/api/files/collab/doc/{page_uuid}/{}",
            src.diagram.rsplit('/').next().unwrap_or_default()
        ),
        "the document's image names the page's new uuid"
    );

    let storage = WorkspaceScopedStorage::arc(test_storage(), t);
    let asset = id_in(
        &mut pool.get().expect("conn"),
        "SELECT id FROM assets WHERE workspace_id = $1 AND name = 'Projector'",
        t,
    );
    let moved_asset = |key: &str| {
        key.replacen(
            &format!("assets/{}/", src.seed_asset()),
            &format!("assets/{asset}/"),
            1,
        )
    };
    for (key, what) in [
        (src.attachment.clone(), "attachment"),
        (moved_note.clone(), "notes image"),
        (
            diagram_src.trim_start_matches("/api/files/").to_string(),
            "document image",
        ),
        (moved_asset(&src.photo), "asset photo"),
        (moved_asset(&src.thumb), "asset thumbnail"),
        ("branding/logo.png".to_string(), "logo"),
    ] {
        assert!(
            storage.file_exists(&key).await.unwrap_or(false),
            "{what} at {key}"
        );
    }

    // Settings carry over; sealed values are dropped and flagged;
    // integrations arrive off.
    assert_eq!(
        text(&pool, "SELECT app_name || ' ' || primary_color AS value FROM site_settings WHERE workspace_id = $1", t).as_deref(),
        Some("Acme Desk #123456")
    );
    assert_eq!(
        text(&pool, "SELECT encrypted_smtp_password::text AS value FROM workspace_email_settings WHERE workspace_id = $1", t),
        None,
        "the sealed SMTP password stays behind"
    );
    let handover = serde_json::to_value(&applied.report.handover).expect("handover");
    assert!(
        handover
            .as_array()
            .is_some_and(|items| items.iter().any(|h| {
                h["kind"] == "encrypted_setting_dropped"
                    && h["table"] == "workspace_email_settings"
                    && h["column"] == "encrypted_smtp_password"
            })),
        "the dropped password is flagged: {handover}"
    );
    assert_eq!(
        text(
            &pool,
            "SELECT concat_ws(',', \
               (SELECT bool_or(enabled) FROM channels WHERE workspace_id = $1), \
               (SELECT bool_or(enabled) FROM webhooks WHERE workspace_id = $1), \
               (SELECT string_agg(state::text, ',') FROM plugins WHERE workspace_id = $1), \
               (SELECT bool_or(enabled) FROM workspace_email_settings WHERE workspace_id = $1)) AS value",
            t
        )
        .as_deref(),
        Some("f,f,disabled,f"),
        "channels, webhooks, plugins and outbound email arrive off"
    );

    // People: those with an email map to their accounts, who keep their
    // own avatar; the rest are created, with their avatar copied to keys of
    // their own.
    let person = |uuid: Uuid| {
        applied
            .report
            .people
            .iter()
            .find(|p| p.archive_uuid == uuid)
            .unwrap_or_else(|| panic!("{uuid} in the people map"))
            .clone()
    };
    for p in [&src.ana, &src.rex] {
        let mapped = person(p.uuid);
        assert_eq!(
            (mapped.outcome, mapped.target_uuid),
            (Outcome::Mapped, p.uuid),
            "{}",
            p.name
        );
    }
    let ana_avatar = src.ana.avatar.clone().expect("Ana's avatar");
    assert_eq!(
        text(
            &pool,
            &format!(
                "SELECT avatar_url AS value FROM users WHERE uuid = '{}' AND $1 = $1",
                src.ana.uuid
            ),
            t
        ),
        Some(ana_avatar.0.clone()),
        "a mapped account keeps its avatar"
    );
    assert_eq!(
        test_storage()
            .get_file(ana_avatar.0.trim_start_matches("/uploads/"))
            .await
            .expect("Ana's avatar file"),
        ana_avatar.1
    );
    let admin = person(src.admin.uuid);
    assert_eq!(admin.outcome, Outcome::Created);
    assert_ne!(
        admin.target_uuid, src.admin.uuid,
        "the source still holds the uuid"
    );
    let avatar_url = text(
        &pool,
        &format!(
            "SELECT avatar_url AS value FROM users WHERE uuid = '{}' AND $1 = $1",
            admin.target_uuid
        ),
        t,
    )
    .expect("the created account has an avatar");
    assert_eq!(
        avatar_url,
        format!("/uploads/users/avatars/{}_avatar.png", admin.target_uuid)
    );
    assert_eq!(
        test_storage()
            .get_file(avatar_url.trim_start_matches("/uploads/"))
            .await
            .expect("the copied avatar"),
        src.admin.avatar.clone().expect("admin avatar").1
    );
    let thumb = text(
        &pool,
        &format!(
            "SELECT avatar_thumb AS value FROM users WHERE uuid = '{}' AND $1 = $1",
            admin.target_uuid
        ),
        t,
    )
    .expect("a thumbnail");
    assert!(
        test_storage()
            .file_exists(thumb.trim_start_matches("/uploads/"))
            .await
            .unwrap_or(false),
        "the thumbnail is made: {thumb}"
    );
    assert_eq!(
        text(
            &pool,
            "SELECT string_agg(role::text, ',' ORDER BY role::text) AS value FROM workspace_members WHERE workspace_id = $1",
            t
        )
        .as_deref(),
        Some("admin,agent,member,member,owner"),
        "everyone keeps their role, and the owner stays"
    );

    // Restoring again puts nothing.
    let again = files::restore(test_storage(), t, &first, &applied.files).await;
    assert_eq!(
        (again.restored, again.failed.as_deref()),
        (0, None),
        "{again:?}"
    );

    // The second archive matches the first, ids aside.
    let second_bytes = export(&pool, &src.seed, t).await;
    let second = read_archive(&second_bytes, Some(PASSWORD)).expect("read the second archive");
    let differences = compare(&first, &second);
    assert!(
        differences.is_empty(),
        "the archives differ:\n{}",
        differences.join("\n")
    );

    // And the same archive imports again on this server.
    let again = fresh_target(&pool);
    let (applied, restored) = import(&pool, &first, &again).await;
    assert!(
        applied.report.refusals.is_empty(),
        "{:?}",
        applied.report.refusals
    );
    assert!(
        restored.failed.is_none() && restored.skipped.is_empty(),
        "{restored:?}"
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*)::bigint AS n FROM outbound_emails WHERE workspace_id = $1",
            again.id
        ),
        1
    );
}

impl Source {
    /// The source's first ticket id, from the notes image key.
    fn seed_ticket_one(&self) -> String {
        self.note.split('/').nth(1).unwrap_or_default().to_string()
    }

    fn seed_asset(&self) -> String {
        self.photo.split('/').nth(1).unwrap_or_default().to_string()
    }
}

// ---------------------------------------------------------------------------
// Comparing archives, ids aside
// ---------------------------------------------------------------------------

static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        .expect("uuid pattern")
});
static OWNED_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(tickets|assets)/\d+/").expect("path pattern"));
static ID_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(data-ticket-id)="\d+""#).expect("attr pattern"));
/// A ticket route, by number or by id: a rewritten link takes the id route.
static TICKET_ROUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/tickets/(id/)?\d+").expect("route pattern"));

/// A column whose value is a key or a reference: it differs between the
/// archives by construction.
fn is_key(column: &str) -> bool {
    column == "id"
        || column == "ticketId"
        || column == "uuid"
        || column.ends_with("_id")
        || column.ends_with("_ids")
        || column.ends_with("_uuid")
        || column.ends_with("_by")
}

/// Columns the import sets on purpose, per table.
fn set_by_import(table: &str, column: &str) -> bool {
    matches!(
        (table, column),
        ("tickets", "guest_lookup_token")
            | ("outbound_emails", "message_id")
            | (_, "enabled")
            | ("plugins", "state")
            | (_, "teams_enabled")
            | ("webhooks", "secret")
            // The import points the branding at the target's own address.
            | ("site_settings", "updated_at")
    ) || column.starts_with("encrypted_")
}

fn canonical_text(s: &str) -> String {
    let s = UUID.replace_all(s, "<uuid>");
    let s = OWNED_PATH.replace_all(&s, "$1/<id>/");
    let s = TICKET_ROUTE.replace_all(&s, "/tickets/<ref>");
    ID_ATTR.replace_all(&s, "$1=\"<id>\"").into_owned()
}

fn canonical(value: &Value, key: Option<&str>) -> Value {
    match value {
        Value::String(s)
            if key.is_some_and(is_key)
                && !s.is_empty()
                && s.bytes().all(|b| b.is_ascii_digit()) =>
        {
            json!("<id>")
        }
        Value::String(s) => Value::String(canonical_text(s)),
        Value::Number(_) if key.is_some_and(|k| is_key(k) || k == "id") => json!("<id>"),
        Value::Array(items) => Value::Array(items.iter().map(|v| canonical(v, key)).collect()),
        Value::Object(map) => {
            // A view predicate's value is a key when its field is one.
            let field = map
                .get("field")
                .and_then(Value::as_str)
                .filter(|f| is_key(f) || f.ends_with(".id"));
            Value::Object(
                map.iter()
                    .map(|(k, v)| {
                        let key = if k == "value" {
                            field.or(Some(k))
                        } else {
                            Some(k.as_str())
                        };
                        (k.clone(), canonical(v, key))
                    })
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

/// A row as both archives must agree on it.
fn canonical_row(table: &str, row: &Value) -> String {
    let Some(map) = row.as_object() else {
        return row.to_string();
    };
    let mut out = serde_json::Map::new();
    for (column, value) in map {
        // A state vector changes with any rewrite of its document.
        if is_key(column) || set_by_import(table, column) || column.ends_with("state_vector") {
            continue;
        }
        let document = value
            .as_str()
            .and_then(|hex| hex.strip_prefix("\\x"))
            .and_then(hex_bytes)
            .filter(|_| column.starts_with("yjs_") || column.ends_with("_snapshot"));
        // A document: compare what it says.
        let value = match document {
            Some(bytes) => canonical(&decode::to_json(&bytes), None),
            None => canonical(value, Some(column)),
        };
        out.insert(column.clone(), value);
    }
    Value::Object(out).to_string()
}

fn hex_bytes(hex: &str) -> Option<Vec<u8>> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}

/// `row` of `table` in the first archive as the import means to change it:
/// queued mail stays behind, and a reference to CSV import history (never
/// carried) is cleared. `None`: the row doesn't arrive.
fn as_imported(table: &str, row: &Value) -> Option<Value> {
    let mut row = row.clone();
    match table {
        "outbound_emails"
            if row["status"]
                .as_str()
                .is_some_and(|s| ["pending", "sending", "failed"].contains(&s)) =>
        {
            return None;
        }
        "asset_lifecycle_events" => {
            if let Some(metadata) = row
                .get_mut("metadata")
                .and_then(Value::as_object_mut)
                .filter(|m| m.contains_key("import_job_id"))
            {
                metadata.insert("import_job_id".into(), Value::Null);
            }
        }
        _ => {}
    }
    Some(row)
}

/// Whether `row` of `table` in the second archive is the target's own,
/// there before the import: its owner and their membership.
fn targets_own(table: &str, row: &Value) -> bool {
    match table {
        "users" => row["name"] == "Target Owner",
        "workspace_members" => row["role"] == "owner",
        _ => false,
    }
}

/// The ways `second` differs from `first`, ids aside: per table, rows only
/// one of them holds.
fn compare(first: &ArchiveContents, second: &ArchiveContents) -> Vec<String> {
    let tables: BTreeSet<&String> = first.tables.keys().chain(second.tables.keys()).collect();
    let mut out = Vec::new();
    for table in tables {
        if matches!(table.as_str(), "workspaces") {
            continue;
        }
        let mut a: BTreeMap<String, usize> = BTreeMap::new();
        for row in first.tables.get(table).into_iter().flatten() {
            if let Some(row) = as_imported(table, row) {
                *a.entry(canonical_row(table, &row)).or_default() += 1;
            }
        }
        let mut b: BTreeMap<String, usize> = BTreeMap::new();
        for row in second.tables.get(table).into_iter().flatten() {
            if !targets_own(table, row) {
                *b.entry(canonical_row(table, row)).or_default() += 1;
            }
        }
        if a == b {
            continue;
        }
        for (row, n) in &a {
            let m = b.get(row).copied().unwrap_or(0);
            if m < *n {
                out.push(format!("{table}: only in the first ({}): {row}", n - m));
            }
        }
        for (row, n) in &b {
            let m = a.get(row).copied().unwrap_or(0);
            if m < *n {
                out.push(format!("{table}: only in the second ({}): {row}", n - m));
            }
        }
    }
    let files = |c: &ArchiveContents, prefix: &str| -> BTreeSet<String> {
        c.files
            .iter()
            .map(|(n, _)| canonical_text(n.strip_prefix(prefix).unwrap_or(n)))
            .collect()
    };
    let (a, b) = (files(first, "files/"), files(second, "files/"));
    if a != b {
        out.push(format!(
            "files: {:?} vs {:?}",
            a.symmetric_difference(&b).collect::<Vec<_>>(),
            ()
        ));
    }
    let avatars = |c: &ArchiveContents| -> BTreeSet<Vec<u8>> {
        c.avatars.iter().map(|(_, b)| b.clone()).collect()
    };
    if avatars(first) != avatars(second) {
        out.push("avatars: the files differ".to_string());
    }
    out
}

// ---------------------------------------------------------------------------
// Damaged archives
// ---------------------------------------------------------------------------

/// `archive` (a plaintext zip) with `edit` applied to its entries.
fn rewrite(archive: &[u8], edit: impl Fn(&str, Vec<u8>) -> Option<(String, Vec<u8>)>) -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(archive.to_vec())).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut entry = source.by_index(i).expect("entry");
        let name = entry.name().expect("entry name").to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read entry");
        if let Some((name, bytes)) = edit(&name, bytes) {
            out.start_file(name, zip::write::SimpleFileOptions::default())
                .expect("start");
            out.write_all(&bytes).expect("write");
        }
    }
    out.finish().expect("finish").into_inner()
}

fn corrupted(result: Result<ArchiveContents, BackupError>) -> String {
    match result {
        Err(BackupError::CorruptedBackup(message)) => message,
        Err(other) => panic!("expected a corrupted archive, got {other}"),
        Ok(_) => panic!("expected a corrupted archive, it was read"),
    }
}

/// A changed table, a table taken out, a table the manifest doesn't list
/// and a truncated file are each refused when the archive is read.
#[actix_web::test]
async fn a_damaged_archive_is_refused() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seed = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    exec(
        &mut pool.get().expect("conn"),
        "INSERT INTO tickets (workspace_id, title, workflow_state_id) VALUES ($1, 'Intact', \
           (SELECT id FROM workflow_states WHERE workspace_id = $1 ORDER BY id LIMIT 1))",
        seed.workspace_id,
    );
    let (status, body) = call_admin_route(
        &pool,
        &seed,
        http_test::TestRequest::post()
            .uri(&format!(
                "/api/admin/workspaces/{}/export",
                seed.workspace_id
            ))
            .set_json(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let archive = body.to_vec();
    read_archive(&archive, None).expect("the archive as exported reads");

    let changed = rewrite(&archive, |name, bytes| {
        let bytes = if name == "data/tickets.json" {
            String::from_utf8(bytes)
                .expect("utf8")
                .replace("Intact", "Altered")
                .into_bytes()
        } else {
            bytes
        };
        Some((name.to_string(), bytes))
    });
    let message = corrupted(read_archive(&changed, None));
    assert!(
        message.contains("checksum mismatch for data/tickets.json"),
        "{message}"
    );

    let removed = rewrite(&archive, |name, bytes| {
        (name != "data/tickets.json").then(|| (name.to_string(), bytes))
    });
    let message = corrupted(read_archive(&removed, None));
    assert!(
        message.contains("data/tickets.json is missing"),
        "{message}"
    );

    let unlisted = rewrite(&archive, |name, bytes| {
        let bytes = if name == "manifest.json" {
            let mut manifest: Value = serde_json::from_slice(&bytes).expect("manifest");
            manifest["tables"]
                .as_object_mut()
                .expect("tables")
                .remove("tickets");
            serde_json::to_vec(&manifest).expect("json")
        } else {
            bytes
        };
        Some((name.to_string(), bytes))
    });
    let message = corrupted(read_archive(&unlisted, None));
    assert!(
        message.contains("data/tickets.json isn't in the manifest"),
        "{message}"
    );

    let truncated = &archive[..archive.len() / 2];
    assert!(
        matches!(read_archive(truncated, None), Err(BackupError::ZipError(_))),
        "a truncated archive doesn't open"
    );
}
