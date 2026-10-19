//! A format 2 workspace export carries each person as a fixed projection with
//! their primary email and avatar, sealed or not without a password hash or
//! MFA secret, and imports back as a new workspace whose new people can sign
//! in by that email.

use actix_web::http::StatusCode;
use actix_web::test as http_test;
use diesel::prelude::*;
use diesel::sql_types::{Bool, Integer, Nullable, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::services::workspace_archive::policy::ARCHIVE_USER_COLUMNS;
use backend::services::workspace_import::{read_archive, ArchiveContents};

use crate::common::{insert_plain_user, seed_two_workspaces, test_storage, TestDb, TestPool};
use crate::workspace_export_hygiene::call_admin_route;

const PASSWORD: &str = "export-v2-password";
const MFA_SECRET_HEX: &str = "5345435245544d4641";
const PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2VjcmV0aGFzaA$c2VjcmV0aGFzaHZhbHVl";

struct Person {
    uuid: Uuid,
    primary: String,
    secondary: String,
    avatar_key: String,
}

fn add_person(pool: &TestPool, workspace_id: i32) -> Person {
    let mut conn = pool.get().expect("conn");
    let uuid = insert_plain_user(&mut conn, "Exported Person");
    let tag = Uuid::now_v7().simple().to_string();
    let primary = format!("primary-{tag}@example.com");
    let secondary = format!("secondary-{tag}@example.com");
    let avatar_key = format!("users/avatars/{uuid}_avatar.webp");
    diesel::sql_query(format!(
        "UPDATE users SET mfa_enabled = true, mfa_secret = '\\x{MFA_SECRET_HEX}'::bytea, \
         mfa_secret_kek_id = 1, avatar_url = '/uploads/{avatar_key}' WHERE uuid = $1"
    ))
    .bind::<SqlUuid, _>(uuid)
    .execute(&mut conn)
    .expect("mfa and avatar");
    diesel::sql_query(
        "INSERT INTO workspace_members (workspace_id, user_uuid, role) VALUES ($1, $2, 'member')",
    )
    .bind::<Integer, _>(workspace_id)
    .bind::<SqlUuid, _>(uuid)
    .execute(&mut conn)
    .expect("membership");
    for (email, is_primary) in [(&primary, true), (&secondary, false)] {
        diesel::sql_query(
            "INSERT INTO user_emails (user_uuid, email, is_primary, is_verified) \
             VALUES ($1, $2, $3, true)",
        )
        .bind::<SqlUuid, _>(uuid)
        .bind::<Text, _>(email)
        .bind::<Bool, _>(is_primary)
        .execute(&mut conn)
        .expect("email");
    }
    // A directory identity scoped to the workspace, with a password hash.
    diesel::sql_query(
        "INSERT INTO user_auth_identities \
           (user_uuid, provider_type, external_id, email, password_hash, metadata, workspace_id) \
         VALUES ($1, 'ldap', $2, $3, $4, '{\"dn\": \"cn=person\"}'::jsonb, $5)",
    )
    .bind::<SqlUuid, _>(uuid)
    .bind::<Text, _>(format!("cn=person-{tag}"))
    .bind::<Text, _>(&primary)
    .bind::<Text, _>(PASSWORD_HASH)
    .bind::<Integer, _>(workspace_id)
    .execute(&mut conn)
    .expect("identity");
    Person {
        uuid,
        primary,
        secondary,
        avatar_key,
    }
}

async fn export(
    pool: &TestPool,
    ws: &crate::common::WorkspaceSeed,
    password: Option<&str>,
) -> Vec<u8> {
    let (status, body) = call_admin_route(
        pool,
        ws,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/workspaces/{}/export", ws.workspace_id))
            .set_json(serde_json::json!({ "password": password })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    body.to_vec()
}

/// Every row text of every `data/` entry, for "this value is nowhere" checks.
fn all_rows(contents: &ArchiveContents) -> String {
    serde_json::to_string(&contents.tables).expect("rows")
}

#[actix_web::test]
async fn an_export_carries_people_without_credentials_and_imports() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let storage = test_storage();
    let seeded = seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = &seeded.a;
    let person = add_person(&pool, ws.workspace_id);
    storage
        .put_file(b"avatar-bytes", &person.avatar_key, "image/webp")
        .await
        .expect("store avatar");
    // A second person whose avatar file is gone.
    let no_file = add_person(&pool, ws.workspace_id);

    for password in [None, Some(PASSWORD)] {
        let archive = export(&pool, ws, password).await;
        let contents = read_archive(&archive, password).expect("read archive");
        let manifest = &contents.manifest;
        assert_eq!(manifest.workspace_export_format_version, 2);
        assert_eq!(manifest.sealed, password.is_some());
        assert!(manifest.migration_version.is_some() && manifest.schema_sha256.is_some());

        // People: the projection only, sealed or not.
        let users = &contents.tables["users"];
        let row = users
            .iter()
            .find(|u| u["uuid"] == person.uuid.to_string())
            .expect("the person is exported");
        let mut keys: Vec<&str> = row
            .as_object()
            .expect("user row")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = ARCHIVE_USER_COLUMNS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected, "sealed: {}", password.is_some());
        assert_eq!(
            row["avatar_url"],
            format!("/uploads/{}", person.avatar_key),
            "the avatar goes with its file"
        );
        let gone = users
            .iter()
            .find(|u| u["uuid"] == no_file.uuid.to_string())
            .expect("the second person is exported");
        assert!(gone["avatar_url"].is_null(), "no file, no avatar_url");

        // Primary email only.
        let emails: Vec<_> = contents.tables["user_emails"]
            .iter()
            .filter(|e| e["user_uuid"] == person.uuid.to_string())
            .collect();
        assert_eq!(emails.len(), 1, "{emails:?}");
        assert_eq!(emails[0]["email"], person.primary.as_str());
        assert_eq!(emails[0]["is_primary"], true);
        let rows = all_rows(&contents);
        assert!(!rows.contains(&person.secondary), "no secondary address");

        // No password hash or MFA secret anywhere, sealed or not.
        let identity = contents.tables["user_auth_identities"]
            .iter()
            .find(|i| i["user_uuid"] == person.uuid.to_string())
            .expect("the scoped identity is exported");
        assert!(identity.get("password_hash").is_none() && identity.get("metadata").is_none());
        assert!(!rows.contains("argon2id"), "no password hash");
        assert!(!rows.contains(MFA_SECRET_HEX), "no MFA secret");
        assert!(!rows.contains("mfa_secret"), "no MFA columns");

        // The avatar file, outside the workspace's files.
        let avatar = format!(
            "avatars/{}",
            person.avatar_key.trim_start_matches("users/avatars/")
        );
        if password.is_none() {
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive.clone())).expect("zip");
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(
                &mut zip.by_name(&avatar).expect("avatar entry"),
                &mut bytes,
            )
            .expect("read avatar");
            assert_eq!(bytes, b"avatar-bytes");
        }
        assert_eq!(manifest.avatars.total_count, 1);
        assert!(
            !contents
                .files
                .iter()
                .any(|(name, _)| name.contains("avatars/")),
            "avatars aren't workspace files"
        );
    }

    // The sealed archive imports as a new workspace; the person, gone from
    // this server meanwhile, comes back with their primary email only and
    // without MFA or a password.
    let archive = export(&pool, ws, Some(PASSWORD)).await;
    {
        // Both people go: their identities are unique on this server.
        let mut conn = pool.get().expect("conn");
        for uuid in [person.uuid, no_file.uuid] {
            diesel::sql_query("DELETE FROM workspace_members WHERE user_uuid = $1")
                .bind::<SqlUuid, _>(uuid)
                .execute(&mut conn)
                .expect("remove membership");
            diesel::sql_query("DELETE FROM users WHERE uuid = $1")
                .bind::<SqlUuid, _>(uuid)
                .execute(&mut conn)
                .expect("remove person");
        }
    }
    let mut payload = b"--x\r\nContent-Disposition: form-data; name=\"archive\"; filename=\"ws.nosdesk\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
    payload.extend_from_slice(&archive);
    payload.extend_from_slice(
        format!(
            "\r\n--x\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\n{PASSWORD}\
             \r\n--x\r\nContent-Disposition: form-data; name=\"regenerate_uuids\"\r\n\r\ntrue\
             \r\n--x\r\nContent-Disposition: form-data; name=\"slug\"\r\n\r\nv2-{}\
             \r\n--x--\r\n",
            Uuid::now_v7().simple()
        )
        .as_bytes(),
    );
    let (status, body) = call_admin_route(
        &pool,
        ws,
        http_test::TestRequest::post()
            .uri("/api/admin/workspaces/import")
            .insert_header(("content-type", "multipart/form-data; boundary=x"))
            .set_payload(payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let result: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let imported = result["workspace_id"].as_i64().expect("workspace id") as i32;

    #[derive(QueryableByName)]
    struct Email {
        #[diesel(sql_type = Text)]
        email: String,
        #[diesel(sql_type = Bool)]
        is_primary: bool,
    }
    #[derive(QueryableByName)]
    struct Back {
        #[diesel(sql_type = Bool)]
        mfa_enabled: bool,
        #[diesel(sql_type = Nullable<Text>)]
        avatar_url: Option<String>,
    }
    #[derive(QueryableByName)]
    struct Identity {
        #[diesel(sql_type = Nullable<Text>)]
        password_hash: Option<String>,
    }
    let mut conn = pool.get().expect("conn");
    let emails: Vec<Email> =
        diesel::sql_query("SELECT email, is_primary FROM user_emails WHERE user_uuid = $1")
            .bind::<SqlUuid, _>(person.uuid)
            .load(&mut conn)
            .expect("emails");
    assert_eq!(emails.len(), 1);
    assert_eq!(emails[0].email, person.primary);
    assert!(emails[0].is_primary);
    let back: Back = diesel::sql_query("SELECT mfa_enabled, avatar_url FROM users WHERE uuid = $1")
        .bind::<SqlUuid, _>(person.uuid)
        .get_result(&mut conn)
        .expect("the person is imported");
    assert!(!back.mfa_enabled);
    assert_eq!(
        back.avatar_url,
        Some(format!("/uploads/{}", person.avatar_key))
    );
    let identity: Identity = diesel::sql_query(
        "SELECT password_hash FROM user_auth_identities WHERE user_uuid = $1 AND workspace_id = $2",
    )
    .bind::<SqlUuid, _>(person.uuid)
    .bind::<Integer, _>(imported)
    .get_result(&mut conn)
    .expect("the identity is imported");
    assert_eq!(identity.password_hash, None);
}

/// An import of an export another release made is refused with the message
/// naming the release to run, not as an invalid archive.
#[actix_web::test]
async fn an_export_from_another_release_is_refused_by_name() {
    use std::io::{Read, Write};

    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let _ = test_storage();
    let seeded = seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = &seeded.a;
    let archive = export(&pool, ws, None).await;

    // Rewrite the manifest as a 1.1.1 export would have it.
    let mut source = zip::ZipArchive::new(std::io::Cursor::new(archive)).expect("zip");
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut entry = source.by_index(i).expect("entry");
        let name = entry.name().expect("entry name").to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read entry");
        if name == "manifest.json" {
            let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
            let point = backend::services::backup_upgrade::MIGRATION_PREFIXES
                .iter()
                .find(|p| p.version == "20261014060000")
                .expect("the 1.1.1 point");
            manifest["migration_version"] = point.version.into();
            manifest["schema_sha256"] = point.sha256.into();
            bytes = serde_json::to_vec(&manifest).expect("json");
        }
        out.start_file(name, zip::write::SimpleFileOptions::default())
            .expect("start");
        out.write_all(&bytes).expect("write");
    }
    let tampered = out.finish().expect("finish").into_inner();

    let mut payload = b"--x\r\nContent-Disposition: form-data; name=\"archive\"; filename=\"ws.nosdesk\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
    payload.extend_from_slice(&tampered);
    payload.extend_from_slice(b"\r\n--x--\r\n");
    let (status, body) = call_admin_route(
        &pool,
        ws,
        http_test::TestRequest::post()
            .uri("/api/admin/workspaces/import")
            .insert_header(("content-type", "multipart/form-data; boundary=x"))
            .set_payload(payload),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body = String::from_utf8_lossy(&body);
    assert!(
        body.contains("This export was made by Nosdesk 1.1.1")
            && body.contains("import it on a server running Nosdesk 1.1.1"),
        "{body}"
    );
    assert!(!body.contains("invalid archive"), "{body}");
}
