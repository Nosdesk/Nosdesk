//! Documentation export honours the page ACL, on the page and on its embeds.
//!
//! `export_page_as_markdown` used to read a page and follow every
//! `embedded_document` node in it without consulting
//! `can_user_access_page`, which the ordinary page read does consult. The
//! interesting half is the embed: a page the caller may read can embed one
//! they may not, so checking only the top-level page checks the wrong thing.
//!
//! These drive `yjs_to_markdown_with_embeds` directly, the way
//! `portal_session` drives the portal primitives, because that function is
//! where the audience travels with the recursion.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use yrs::types::xml::XmlIn;
use yrs::{Doc, ReadTxn, Transact, WriteTxn, XmlElementPrelim, XmlFragment};

use backend::db::DbConnection;
use backend::models::{DocumentationStatus, NewDocumentationPage, User};
use backend::repository::PageAudience;
use backend::utils::markdown_export::{yjs_to_markdown_with_embeds, EmbedResolver};

mod common;

/// A y-prosemirror fragment holding `text`, built by the converter the app
/// itself uses so the fixture cannot drift from the real document shape.
fn page_body(text: &str) -> Vec<u8> {
    backend::services::seed::markdown_to_yjs(text).expect("build body")
}

/// A fragment that embeds `child_uuid`, the shape the editor writes.
fn page_embedding(child_uuid: uuid::Uuid, child_title: &str) -> Vec<u8> {
    let doc = Doc::new();
    let fragment = {
        let mut txn = doc.transact_mut();
        txn.get_or_insert_xml_fragment("prosemirror")
    };
    {
        let mut txn = doc.transact_mut();
        let mut embed = XmlElementPrelim::new("embedded_document", Vec::<XmlIn>::new());
        embed
            .attributes
            .insert("documentUuid".into(), child_uuid.to_string());
        embed
            .attributes
            .insert("documentTitle".into(), child_title.to_string());
        fragment.push_back(&mut txn, embed);
    }
    let txn = doc.transact();
    txn.encode_state_as_update_v1(&yrs::StateVector::default())
}

fn insert_page(
    conn: &mut DbConnection,
    author: &User,
    title: &str,
    slug: &str,
    body: Vec<u8>,
) -> backend::models::DocumentationPage {
    use backend::schema::documentation_pages;
    let new = NewDocumentationPage {
        uuid: uuid::Uuid::new_v4(),
        title: title.to_string(),
        slug: slug.to_string(),
        icon: None,
        cover_image: None,
        status: DocumentationStatus::Published,
        created_by: author.uuid,
        last_edited_by: author.uuid,
        parent_id: None,
        display_order: None,
        is_public: false,
        is_template: false,
        yjs_state_vector: None,
        yjs_document: Some(body),
        yjs_client_id: None,
        has_unsaved_changes: false,
    };
    diesel::insert_into(documentation_pages::table)
        .values(&new)
        .get_result(conn)
        .expect("insert page")
}

/// Give the page an explicit visibility override naming somebody else, which
/// is the cheapest way to make `can_user_access_page` say no: any override at
/// all switches the page from "inherit" to "listed users and groups only".
fn restrict_page_to(conn: &mut DbConnection, page_id: i32, workspace_id: i32, grantee: uuid::Uuid) {
    use backend::schema::documentation_page_visibility as v;
    diesel::insert_into(v::table)
        .values((
            v::page_id.eq(page_id),
            v::user_uuid.eq(Some(grantee)),
            v::workspace_id.eq(workspace_id),
        ))
        .execute(conn)
        .expect("restrict page");
}

struct Fixture {
    parent_body: Vec<u8>,
    secret_title: String,
    secret_text: String,
    reader: uuid::Uuid,
}

fn setup(conn: &mut DbConnection, slug: &str) -> Fixture {
    let workspace_id = common::mint_workspace(conn, slug, slug);
    backend::sync::session::pin_workspace(conn, workspace_id).expect("pin");
    let author = common::insert_user(conn, "Doc Author");
    let reader = common::insert_user(conn, "Doc Reader");
    let other = common::insert_user(conn, "Someone Else");

    let secret_text = "the embedded secret".to_string();
    let secret = insert_page(
        conn,
        &author,
        "Secret Page",
        &format!("{slug}-secret"),
        page_body(&secret_text),
    );
    restrict_page_to(conn, secret.id, workspace_id, other.uuid);

    let parent_body = page_embedding(secret.uuid, "Secret Page");
    Fixture {
        parent_body,
        secret_title: "Secret Page".to_string(),
        secret_text,
        reader: reader.uuid,
    }
}

fn render(conn: &mut DbConnection, body: &[u8], audience: PageAudience) -> String {
    let mut visited = std::collections::HashSet::new();
    let locale: unic_langid::LanguageIdentifier = "en-US".parse().expect("locale");
    let mut resolver = EmbedResolver { conn, audience };
    yjs_to_markdown_with_embeds(body, &mut resolver, &mut visited, None, 0, &locale)
        .unwrap_or_default()
}

#[test]
fn an_export_omits_an_embedded_page_the_reader_cannot_open() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    let f = setup(&mut conn, "acme-embed-denied");

    let out = render(
        &mut conn,
        &f.parent_body,
        PageAudience::User {
            user_uuid: f.reader,
            is_admin: false,
        },
    );

    assert!(
        !out.contains(&f.secret_text),
        "an embedded page outside the reader's ACL must not be rendered: {out}"
    );
    assert!(
        out.contains(&f.secret_title),
        "the fallback reference still names the page, which the embedding \
         document already contained: {out}"
    );
}

#[test]
fn an_export_includes_an_embedded_page_the_reader_can_open() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    let f = setup(&mut conn, "acme-embed-allowed");

    // Same document, same fixture, admin audience: proves the omission above
    // is the ACL talking and not the embed machinery failing to resolve.
    let out = render(
        &mut conn,
        &f.parent_body,
        PageAudience::User {
            user_uuid: f.reader,
            is_admin: true,
        },
    );

    assert!(
        out.contains(&f.secret_text),
        "an admin's export must still resolve the embed: {out}"
    );
}
