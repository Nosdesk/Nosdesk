//! On hosted, a helpdesk import seats no staff (the control plane owns staff
//! seats), and what it writes stays in the workspace it imports into: no
//! account from elsewhere is renamed or given an address, every address it
//! adds is an unverified primary, and the organization and phone it carries
//! land on this workspace's profiles only, with no sign-in rule or sharing
//! setting touched. Driven the way a Cloud import runs: an export zip read
//! with the API fill, against a Zendesk stand-in on 127.0.0.1.
//!
//! Part of the hosted binary rather than `tests/it`: the deployment mode is
//! process-wide environment.

use std::collections::HashMap;

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Integer, Text};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::common;
use backend::services::imports::helpdesk::bundle::{Bundle, PeopleSource};
use backend::services::imports::helpdesk::loader::{self, LoadOptions, NumberPolicy};
use backend::services::imports::helpdesk::zendesk::{read_export, ZendeskAuth, ZendeskSource};

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count(conn: &mut common::TestPooledConn, sql: &str) -> i64 {
    diesel::sql_query(sql)
        .get_result::<Count>(conn)
        .expect("count")
        .n
}

/// What the stand-in answers: an admin's credential, no custom fields, and
/// user 7 by id (8 is gone).
fn answer(path: &str, query: &HashMap<String, String>) -> (u16, Value) {
    match path {
        "/api/v2/users/me.json" => (200, json!({"user": {"id": 99, "role": "admin"}})),
        "/api/v2/ticket_fields.json" => (
            200,
            json!({"ticket_fields": [], "meta": {"has_more": false}}),
        ),
        "/api/v2/users/show_many.json" => {
            let users: Vec<Value> = query
                .get("ids")
                .map(String::as_str)
                .unwrap_or_default()
                .split(',')
                .filter(|id| *id == "7")
                .map(|_| {
                    json!({"id": 7, "name": "Fran Filled", "email": "fran@customer.test",
                           "role": "end-user", "active": true, "phone": "+61277770000"})
                })
                .collect();
            (200, json!({"users": users}))
        }
        "/api/v2/organizations/show_many.json" => (200, json!({"organizations": []})),
        _ => (404, json!({})),
    }
}

/// A minimal HTTP/1.1 stand-in: one GET per connection. Returns its base
/// URL and the paths it was asked for.
async fn stand_in() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log_in = log.clone();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let log = log_in.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let target = head.split(' ').nth(1).unwrap_or("/").to_string();
                let url = url::Url::parse(&format!("http://stand-in{target}")).expect("target");
                let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
                log.lock().expect("log").push(url.path().to_string());
                let (status, body) = answer(url.path(), &query);
                let body = body.to_string();
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (format!("http://127.0.0.1:{port}"), log)
}

/// An account from outside the import, with a verified primary address and
/// a seat in `ws` as `role`.
fn outsider(
    conn: &mut common::TestPooledConn,
    name: &str,
    email: &str,
    ws: i32,
    role: &str,
) -> Uuid {
    let uuid = common::insert_plain_user(conn, name);
    diesel::sql_query(
        "INSERT INTO user_emails (user_uuid, email, email_type, is_primary, is_verified) \
         VALUES ($1, $2, 'personal', true, true)",
    )
    .bind::<diesel::sql_types::Uuid, _>(uuid)
    .bind::<Text, _>(email)
    .execute(conn)
    .expect("address");
    common::seat(conn, ws, uuid, role);
    uuid
}

fn name_of(conn: &mut common::TestPooledConn, user: Uuid) -> String {
    #[derive(QueryableByName)]
    struct Name {
        #[diesel(sql_type = Text)]
        name: String,
    }
    diesel::sql_query("SELECT name FROM users WHERE uuid = $1")
        .bind::<diesel::sql_types::Uuid, _>(user)
        .get_result::<Name>(conn)
        .expect("name")
        .name
}

fn seat_in(conn: &mut common::TestPooledConn, ws: i32, user: Uuid) -> Option<String> {
    #[derive(QueryableByName)]
    struct Role {
        #[diesel(sql_type = Text)]
        role: String,
    }
    diesel::sql_query(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_uuid = $2 \
         AND removed_at IS NULL",
    )
    .bind::<Integer, _>(ws)
    .bind::<diesel::sql_types::Uuid, _>(user)
    .get_result::<Role>(conn)
    .optional()
    .expect("seat")
    .map(|r| r.role)
}

/// The export: three tickets, the users export and the organizations
/// export in one zip. Ticket 11 names 7 and 8, whom no zip holds.
fn export_zip(path: &std::path::Path) {
    use std::io::Write;
    let ticket = |id: i64, requester: i64, extra: Value| {
        let mut t = json!({"ticket": {
            "id": id, "subject": format!("Ticket {id}"), "status": "open",
            "requester_id": requester, "assignee_id": 2, "organization_id": 900,
            "created_at": "2025-03-01T09:00:00Z", "updated_at": "2025-03-01T10:00:00Z",
            "comments": [{"id": id * 10, "author_id": requester, "public": true,
                          "created_at": "2025-03-01T09:00:00Z", "body": "Help"}]
        }});
        if let Some(o) = extra.as_object() {
            for (k, v) in o {
                t["ticket"][k] = v.clone();
            }
        }
        t
    };
    let user = |id: i64, name: &str, email: &str, role: &str| {
        json!({"user": {"id": id, "name": name, "email": email, "role": role,
                        "active": true, "organization_id": 900, "phone": "+61299990000"}})
    };
    let lines = [
        ticket(11, 1, json!({"collaborator_ids": [7, 8]})),
        ticket(12, 3, json!({})),
        ticket(13, 4, json!({})),
        user(1, "Cara", "cara@customer.test", "end-user"),
        user(2, "Alan Agent", "alan@zendesk-staff.test", "admin"),
        user(3, "Bea At Zendesk", "bea@customer.test", "end-user"),
        user(4, "Ezra At Zendesk", "ezra@customer.test", "end-user"),
        json!({"organization": {"id": 900, "name": "Customer Co",
                                "domain_names": ["customer.test"], "shared_tickets": true}}),
    ];
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).expect("zip"));
    zip.start_file("export_1", zip::write::SimpleFileOptions::default())
        .expect("entry");
    for l in lines {
        writeln!(zip, "{l}").expect("line");
    }
    zip.finish().expect("finish");
}

#[actix_web::test]
async fn hosted_import_seats_no_staff_and_writes_nothing_global() {
    crate::hosted_env();
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a, seeded.b);

    // Two accounts of workspace B whose addresses Zendesk users also have:
    // a requester there (within this workspace's reach, so seated here as a
    // requester) and an agent there (beyond it, so nobody's account is
    // touched and their replies go to the placeholder).
    let mut conn = pool.get().expect("conn");
    let bea = outsider(
        &mut conn,
        "Bea Elsewhere",
        "bea@customer.test",
        b.workspace_id,
        "member",
    );
    let ezra = outsider(
        &mut conn,
        "Ezra Elsewhere",
        "ezra@customer.test",
        b.workspace_id,
        "agent",
    );
    let emails_before = count(&mut conn, "SELECT count(*) AS n FROM user_emails");

    let dir = tempfile::tempdir().expect("dir");
    let zip_path = dir.path().join("export.zip");
    export_zip(&zip_path);
    let (base, log) = stand_in().await;
    // The stand-in's host is trusted from when the source is built; the
    // allowlist is put back at once.
    let source = {
        let previous = std::env::var("NOSDESK_OUTBOUND_ALLOWED_HOSTS").ok();
        std::env::set_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS", "127.0.0.1");
        let built = ZendeskSource::with_base_url(&base, ZendeskAuth::Bearer("t".into()));
        match previous {
            Some(v) => std::env::set_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS", v),
            None => std::env::remove_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS"),
        }
        built.expect("stand-in")
    };
    let bundle = Bundle::create(dir.path().join("bundle")).expect("bundle");
    let manifest = read_export(
        &source,
        std::slice::from_ref(&zip_path),
        &bundle,
        &|_| {},
        CancellationToken::new(),
    )
    .await
    .expect("export with the fill");
    assert_eq!(manifest.people_source, Some(PeopleSource::Export));
    let fill = manifest.fill.clone().expect("fill");
    assert_eq!(
        (fill.people_returned, fill.gone.clone()),
        (1, vec!["8".to_string()])
    );
    assert!(
        log.lock()
            .expect("log")
            .iter()
            .all(|p| !p.contains("/incremental/")),
        "{:?}",
        log.lock().expect("log")
    );

    let job = Uuid::now_v7();
    let opts = LoadOptions {
        job_id: job,
        workspace_id: a.workspace_id,
        actor: a.admin_uuid,
        numbers: NumberPolicy::Auto,
        agent_map: HashMap::new(),
        create_agents: true,
        allow_missing_people: false,
        batch_size: 10,
        people_batch_size: 10,
        cancel: Default::default(),
    };
    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert_eq!(plan.refused, None, "the one missing is gone at Zendesk");
    let storage: std::sync::Arc<dyn backend::utils::storage::Storage> =
        std::sync::Arc::new(backend::utils::storage::LocalStorage::new(
            tempfile::tempdir()
                .expect("storage")
                .keep()
                .to_string_lossy()
                .into_owned(),
            "/files".into(),
        ));
    let report = loader::load(&pool, storage, &bundle, &opts, &|_| {})
        .await
        .expect("load");
    assert_eq!(report.tickets_written, 3);

    // One outcome each: Cara and Fran created as requesters, the admin at
    // Zendesk an account without a seat, Bea's account seated here as a
    // requester, Ezra's untouched with their replies on the placeholder.
    let outcomes: Vec<(&str, u64)> = report.people.iter().map(|(k, v)| (*k, *v)).collect();
    assert_eq!(
        outcomes,
        vec![
            ("address held elsewhere (placeholder)", 1),
            ("former staff, no seat", 1),
            ("requester, account from elsewhere", 1),
            ("requester, created", 2),
        ],
        "{:?}",
        report.people
    );
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS n FROM workspace_members m JOIN user_emails e \
                 ON e.user_uuid = m.user_uuid WHERE e.email = 'alan@zendesk-staff.test' \
                 AND m.workspace_id = {}",
                a.workspace_id
            )
        ),
        0,
        "no staff seat"
    );
    assert_eq!(
        seat_in(&mut conn, a.workspace_id, bea).as_deref(),
        Some("member")
    );
    assert_eq!(seat_in(&mut conn, a.workspace_id, ezra), None);

    // Accounts from elsewhere: not renamed, not given an address.
    assert_eq!(name_of(&mut conn, bea), "Bea Elsewhere");
    assert_eq!(name_of(&mut conn, ezra), "Ezra Elsewhere");
    for who in [bea, ezra] {
        assert_eq!(
            count(
                &mut conn,
                &format!("SELECT count(*) AS n FROM user_emails WHERE user_uuid = '{who}'")
            ),
            1
        );
    }
    // Every address the import added is an unverified primary: Cara's,
    // Fran's, Alan's and the placeholder author's undeliverable one.
    assert_eq!(
        count(&mut conn, "SELECT count(*) AS n FROM user_emails") - emails_before,
        4
    );
    #[derive(QueryableByName)]
    struct Addr {
        #[diesel(sql_type = Bool)]
        is_primary: bool,
        #[diesel(sql_type = Bool)]
        is_verified: bool,
    }
    for email in [
        "cara@customer.test",
        "fran@customer.test",
        "alan@zendesk-staff.test",
    ] {
        let row =
            diesel::sql_query("SELECT is_primary, is_verified FROM user_emails WHERE email = $1")
                .bind::<Text, _>(email)
                .get_result::<Addr>(&mut conn)
                .expect("address");
        assert!(row.is_primary && !row.is_verified, "{email}");
    }
    // Profiles and phones only in workspace A: none in B, where the
    // accounts from elsewhere have their seats, and none on Ezra's.
    for table in ["user_profiles", "user_phone_numbers"] {
        assert_eq!(
            count(
                &mut conn,
                &format!(
                    "SELECT count(*) AS n FROM {table} WHERE workspace_id <> {}",
                    a.workspace_id
                )
            ),
            0,
            "{table}"
        );
        assert_eq!(
            count(
                &mut conn,
                &format!("SELECT count(*) AS n FROM {table} WHERE user_uuid = '{ezra}'")
            ),
            0,
            "{table}"
        );
    }
    // Cara's, Alan's and Bea's organization; Fran has none at Zendesk.
    assert_eq!(report.profile.organizations, 3, "{:?}", report.profile);
    // No sign-in rule or sharing setting from the organization's domains.
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS n FROM workspace_identity_providers \
             WHERE cardinality(allowed_domains) > 0"
        ),
        0
    );
    #[derive(QueryableByName)]
    struct Sharing {
        #[diesel(sql_type = Bool)]
        on: bool,
    }
    let sharing = diesel::sql_query(
        "SELECT bool_or(portal_share_by_domain) AS on FROM site_settings WHERE workspace_id = $1",
    )
    .bind::<Integer, _>(a.workspace_id)
    .get_result::<Sharing>(&mut conn)
    .expect("sharing");
    assert!(!sharing.on);
}
