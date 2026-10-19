//! A workspace export imports into an existing workspace nothing has been
//! added to: the workspace keeps its identity, takes the archive's rows,
//! configuration and settings, arrives with its integrations off, and bumps
//! its data epoch so clients start over. A workspace with data of its own
//! refuses with 409, a failed import changes nothing, and a reset takes the
//! workspace back to how it was provisioned.

use actix_web::http::StatusCode;
use actix_web::test as http_test;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Integer, Nullable, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::middleware::workspace_context::DeploymentMode;
use backend::models::NewWorkspace;
use backend::repository::site_settings::name_after_workspace;
use backend::repository::workspaces::{
    add_membership, create_workspace, data_fence, DataFence, SeatWriteAuthority,
};
use backend::services::search::{SearchQuery, SearchService};
use backend::services::seed::{seed_getting_started, seed_workspace_defaults};
use backend::services::workspace_archive::apply::{self, ApplyError, Found};
use backend::services::workspace_archive::plan::{ImportTarget, PlanOptions, Runner};
use backend::services::workspace_import::{read_archive, ArchiveContents};
use backend::sync::actor::ActorContext;
use backend::sync::session::{with_actor_bypass_context, with_actor_context};
use backend::utils::storage::WorkspaceScopedStorage;

use crate::common::{
    insert_plain_user, insert_user, seed_two_workspaces, test_storage, TestDb, TestPool,
    WorkspaceSeed,
};
use crate::workspace_export_hygiene::{call_admin_route, call_admin_route_with_search};

const REF: &str = "test:workspace_import_apply";
const PASSWORD: &str = "apply-password";
const BOUNDARY: &str = "nosdesk-apply-boundary";
const SOURCE_APP_NAME: &str = "Acme Desk";

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count_in(pool: &TestPool, table: &str, workspace_id: i32) -> i64 {
    diesel::sql_query(format!(
        "SELECT count(*)::bigint AS n FROM {table} WHERE workspace_id = $1"
    ))
    .bind::<Integer, _>(workspace_id)
    .get_result::<Count>(&mut pool.get().expect("conn"))
    .expect("count")
    .n
}

fn fence(pool: &TestPool, workspace_id: i32) -> DataFence {
    data_fence(&mut pool.get().expect("conn"), workspace_id).expect("fence")
}

fn app_name(pool: &TestPool, workspace_id: i32) -> String {
    #[derive(QueryableByName)]
    struct Name {
        #[diesel(sql_type = Text)]
        app_name: String,
    }
    diesel::sql_query("SELECT app_name FROM site_settings WHERE workspace_id = $1")
        .bind::<Integer, _>(workspace_id)
        .get_result::<Name>(&mut pool.get().expect("conn"))
        .expect("site settings")
        .app_name
}

/// A workspace row's identity columns, as text.
fn identity(pool: &TestPool, workspace_id: i32) -> Vec<Option<String>> {
    #[derive(QueryableByName)]
    struct Identity {
        #[diesel(sql_type = Text)]
        uuid: String,
        #[diesel(sql_type = Text)]
        slug: String,
        #[diesel(sql_type = Text)]
        name: String,
        #[diesel(sql_type = Text)]
        plan: String,
        #[diesel(sql_type = Nullable<Text>)]
        seat_limit: Option<String>,
        #[diesel(sql_type = Nullable<Text>)]
        custom_domain: Option<String>,
    }
    let row: Identity = diesel::sql_query(
        "SELECT uuid::text AS uuid, slug::text AS slug, name::text AS name, plan::text AS plan, \
         seat_limit::text AS seat_limit, custom_domain FROM workspaces WHERE id = $1",
    )
    .bind::<Integer, _>(workspace_id)
    .get_result(&mut pool.get().expect("conn"))
    .expect("workspace");
    vec![
        Some(row.uuid),
        Some(row.slug),
        Some(row.name),
        Some(row.plan),
        row.seat_limit,
        row.custom_domain,
    ]
}

/// A workspace as hosted provisioning leaves it: created and seeded, its
/// portal named after it (#719, done directly since this binary runs
/// self-hosted), and its owner projected, which seeds the welcome page.
struct Provisioned {
    id: i32,
    name: String,
    owner: Uuid,
}

fn provision_hosted(pool: &TestPool) -> Provisioned {
    let mut conn = pool.get().expect("conn");
    let tag = Uuid::new_v4().simple().to_string()[..8].to_string();
    let name = format!("Apply Target {tag}");
    // Files live in one storage root for the whole test binary, under
    // `ws/{id}/`, and every sandbox database numbers workspaces from 1: a
    // random id keeps this workspace's files its own.
    let id = 1_000_000 + (Uuid::new_v4().as_u128() % 1_000_000_000) as i64;
    diesel::sql_query("SELECT setval(pg_get_serial_sequence('public.workspaces', 'id'), $1)")
        .bind::<BigInt, _>(id)
        .execute(&mut conn)
        .expect("workspace id");
    let ws = create_workspace(
        &mut conn,
        &NewWorkspace {
            uuid: Uuid::now_v7(),
            slug: format!("apply-{tag}"),
            name: name.clone(),
            seat_limit: Some(5),
        },
    )
    .expect("create workspace");
    diesel::sql_query(
        "UPDATE workspaces SET plan = 'trial', custom_domain = $2, settings = '{\"keep\": 1}' \
         WHERE id = $1",
    )
    .bind::<Integer, _>(ws.id)
    .bind::<Text, _>(format!("help-{tag}.example.com"))
    .execute(&mut conn)
    .expect("plan and domain");
    let owner = insert_plain_user(&mut conn, "Hosted Owner");
    let actor = ActorContext::system(REF).with_workspace(ws.id);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        seed_workspace_defaults(c, None)?;
        name_after_workspace(c)?;
        add_membership(c, ws.id, owner, "owner", SeatWriteAuthority::ControlPlane)?;
        seed_getting_started(c, owner)
    })
    .expect("provision");
    Provisioned {
        id: ws.id,
        name,
        owner,
    }
}

/// Workspace 1 after a self-hosted install's setup: its owner, a platform
/// admin, and the welcome page startup seeds for them.
fn set_up_self_hosted(pool: &TestPool) -> Uuid {
    let mut conn = pool.get().expect("conn");
    let admin = insert_user(&mut conn, "Setup Admin").uuid;
    let actor = ActorContext::system(REF).with_workspace(1);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        add_membership(c, 1, admin, "owner", SeatWriteAuthority::Product)?;
        seed_getting_started(c, admin)
    })
    .expect("set up workspace 1");
    admin
}

fn untouched(pool: &TestPool, workspace_id: i32, held: &[String]) -> Vec<Found> {
    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        apply::untouched(c, workspace_id, held)
            .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    })
    .expect("untouched check")
}

/// The source workspace: a ticket with a guest lookup token, a plugin with a
/// secret, outbound email with a sealed password, an enabled webhook and an
/// installed plugin (the fixture's), and a named portal. Exported sealed.
struct Source {
    seed: WorkspaceSeed,
    ticket_title: String,
    guest_token: Uuid,
    archive: Vec<u8>,
}

async fn source(pool: &TestPool) -> Source {
    let seed = {
        let mut conn = pool.get().expect("conn");
        seed_two_workspaces(&mut conn).a
    };
    let ticket_title = format!("Printer on fire {}", Uuid::new_v4().simple());
    let guest_token = Uuid::new_v4();
    {
        let mut conn = pool.get().expect("conn");
        diesel::sql_query(
            "INSERT INTO tickets (workspace_id, title, workflow_state_id, guest_lookup_token) \
             VALUES ($1, $2, (SELECT id FROM workflow_states WHERE workspace_id = $1 \
                              ORDER BY id LIMIT 1), $3)",
        )
        .bind::<Integer, _>(seed.workspace_id)
        .bind::<Text, _>(&ticket_title)
        .bind::<SqlUuid, _>(guest_token)
        .execute(&mut conn)
        .expect("ticket");
        for (key, secret) in [("api-key", true), ("colour", false)] {
            diesel::sql_query(
                "INSERT INTO plugin_data (workspace_id, plugin_id, data_type, key, value, is_secret) \
                 VALUES ($1, $2, 'setting', $3, '\"value\"', $4)",
            )
            .bind::<Integer, _>(seed.workspace_id)
            .bind::<Integer, _>(seed.plugin_id)
            .bind::<Text, _>(key)
            .bind::<Bool, _>(secret)
            .execute(&mut conn)
            .expect("plugin data");
        }
        diesel::sql_query(
            "INSERT INTO workspace_email_settings (workspace_id, enabled, from_name, from_email, \
               smtp_host, smtp_port, smtp_security, smtp_username, encrypted_smtp_password, \
               encrypted_kek_id) \
             VALUES ($1, true, 'Acme', 'help@acme.example', 'smtp.acme.example', 587, \
               'starttls', 'acme', '\\x01020304'::bytea, 1)",
        )
        .bind::<Integer, _>(seed.workspace_id)
        .execute(&mut conn)
        .expect("email settings");
        // A live rule tagging with one of the workspace's tags and one the
        // export doesn't carry.
        diesel::sql_query(
            "WITH tag AS (INSERT INTO tags (workspace_id, name) VALUES ($1, 'vip') RETURNING id) \
             INSERT INTO rules (workspace_id, name, trigger_kind, actions, state) \
             SELECT $1, 'Tag VIPs', 'manual', \
                    jsonb_build_array(jsonb_build_object('kind', 'add_tags', 'config', \
                      jsonb_build_object('tag_ids', jsonb_build_array(tag.id, 2147480000)))), \
                    'live' \
             FROM tag",
        )
        .bind::<Integer, _>(seed.workspace_id)
        .execute(&mut conn)
        .expect("rule");
        diesel::sql_query(
            "INSERT INTO site_settings (workspace_id, app_name, primary_color) \
             VALUES ($1, $2, '#123456') \
             ON CONFLICT (workspace_id) DO UPDATE \
             SET app_name = EXCLUDED.app_name, primary_color = EXCLUDED.primary_color",
        )
        .bind::<Integer, _>(seed.workspace_id)
        .bind::<Text, _>(SOURCE_APP_NAME)
        .execute(&mut conn)
        .expect("portal name");
    }
    let (status, body) = call_admin_route(
        pool,
        &seed,
        http_test::TestRequest::post()
            .uri(&format!(
                "/api/admin/workspaces/{}/export",
                seed.workspace_id
            ))
            .set_json(serde_json::json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    Source {
        seed,
        ticket_title,
        guest_token,
        archive: body.to_vec(),
    }
}

/// A multipart request to the admin import route.
fn import_request(archive: &[u8], fields: &[(&str, &str)]) -> http_test::TestRequest {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"archive\"; \
             filename=\"workspace.nosdesk\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(archive);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    http_test::TestRequest::post()
        .uri("/api/admin/workspaces/import")
        .insert_header((
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        ))
        .set_payload(body)
}

/// `import_into` on the owner role, as the import route runs it.
fn import_into(
    pool: &TestPool,
    contents: &ArchiveContents,
    target: &ImportTarget,
) -> Result<apply::Applied, ApplyError> {
    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        apply::import_into(
            c,
            contents,
            target,
            PlanOptions {
                regenerate_row_uuids: true,
            },
            &[],
        )
    })
}

fn hosted_target(target: &Provisioned) -> ImportTarget {
    ImportTarget {
        workspace_id: target.id,
        mode: DeploymentMode::Hosted,
        runner: Runner::HostedOwner {
            user: target.owner,
            verified_emails: Vec::new(),
        },
    }
}

/// Design §2.2's fingerprint: a workspace as provisioning leaves it, hosted
/// (named portal, projected owner, welcome page) or self-hosted (workspace 1
/// and its setup admin), holds nothing the untouched check counts, and
/// neither do files an export leaves out.
#[actix_web::test]
async fn a_provisioned_workspace_is_untouched() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);

    let hosted = provision_hosted(&pool);
    assert_eq!(app_name(&pool, hosted.id), hosted.name, "#719 names it");
    assert_eq!(untouched(&pool, hosted.id, &[]), Vec::<Found>::new());

    let storage = WorkspaceScopedStorage::arc(test_storage(), hosted.id);
    for path in [
        "imports/0198a0e4-0000-7000-8000-000000000000/upload.csv",
        "exports/workspace.nosdesk",
        "workspace-imports/job.nosdesk",
    ] {
        storage
            .put_file(b"x", path, "application/octet-stream")
            .await
            .expect("put");
    }
    let held = apply::held_files(test_storage(), hosted.id)
        .await
        .expect("list");
    assert!(held.is_empty(), "{held:?}");
    assert_eq!(untouched(&pool, hosted.id, &held), Vec::<Found>::new());

    storage
        .put_file(b"x", "tickets/9/attachments/a.pdf", "application/pdf")
        .await
        .expect("put");
    let held = apply::held_files(test_storage(), hosted.id)
        .await
        .expect("list");
    assert_eq!(
        untouched(&pool, hosted.id, &held),
        vec![Found {
            what: "files".into(),
            rows: 1
        }]
    );

    set_up_self_hosted(&pool);
    assert_eq!(untouched(&pool, 1, &[]), Vec::<Found>::new());
}

/// A workspace with a ticket refuses the import with 409, names what it
/// holds, and is left as it was.
#[actix_web::test]
async fn a_workspace_with_a_ticket_refuses_the_import() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool).await;
    let target = provision_hosted(&pool);
    diesel::sql_query(
        "INSERT INTO tickets (workspace_id, title, workflow_state_id) \
         VALUES ($1, 'Already here', (SELECT id FROM workflow_states WHERE workspace_id = $1 \
                                      ORDER BY id LIMIT 1))",
    )
    .bind::<Integer, _>(target.id)
    .execute(&mut pool.get().expect("conn"))
    .expect("ticket");

    let (status, body) = call_admin_route(
        &pool,
        &src.seed,
        import_request(
            &src.archive,
            &[
                ("password", PASSWORD),
                ("workspace_id", &target.id.to_string()),
                ("regenerate_uuids", "true"),
            ],
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let body: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(body["code"], "workspace_not_empty");
    assert_eq!(
        body["found"],
        serde_json::json!([{ "what": "tickets", "rows": 1 }])
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|m| m.contains("1 tickets")),
        "{body}"
    );

    assert_eq!(count_in(&pool, "tickets", target.id), 1);
    assert_eq!(count_in(&pool, "workflow_states", target.id), 7);
    assert_eq!(fence(&pool, target.id), DataFence::default());

    // Once the ticket is gone the same request imports.
    diesel::sql_query("DELETE FROM tickets WHERE workspace_id = $1")
        .bind::<Integer, _>(target.id)
        .execute(&mut pool.get().expect("conn"))
        .expect("delete the ticket");
    let (status, body) = call_admin_route(
        &pool,
        &src.seed,
        import_request(
            &src.archive,
            &[
                ("password", PASSWORD),
                ("workspace_id", &target.id.to_string()),
                ("regenerate_uuids", "true"),
            ],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let body: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(body["workspace_id"], target.id);
    let fenced = fence(&pool, target.id);
    assert_eq!(body["data_epoch"], serde_json::json!(fenced.epoch));
    assert!(fenced.epoch.is_some());
    assert_eq!(count_in(&pool, "tickets", target.id), 1);
}

/// On self-hosted the import fills workspace 1: it serves the imported
/// tickets, keeps its identity, takes the archive's portal name and settings,
/// leaves sealed credentials and plugin secrets behind, arrives with its
/// integrations off and new guest lookup tokens, and bumps the data epoch.
#[actix_web::test]
async fn importing_into_workspace_one_serves_the_imported_tickets() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool).await;
    set_up_self_hosted(&pool);
    let before = identity(&pool, 1);
    assert_eq!(fence(&pool, 1), DataFence::default());

    // Run directly rather than through the route: workspace 1's files share
    // the binary's storage root with every other test's workspace 1.
    let contents = read_archive(&src.archive, Some(PASSWORD)).expect("read");
    let target = ImportTarget {
        workspace_id: 1,
        mode: DeploymentMode::SelfHosted,
        runner: Runner::Operator { user: None },
    };
    let applied = import_into(&pool, &contents, &target).expect("imports");
    assert_eq!(applied.workspace_id, 1);
    assert!(
        applied.cleared.iter().any(|f| f.what == "workflow_states"),
        "the seeded states are replaced: {:?}",
        applied.cleared
    );

    // Served: the app role pinned to workspace 1 reads the imported ticket.
    #[derive(QueryableByName)]
    struct Title {
        #[diesel(sql_type = Text)]
        title: String,
    }
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::system(REF).with_workspace(1);
    let titles: Vec<String> =
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            diesel::sql_query("SELECT title FROM tickets").load::<Title>(c)
        })
        .expect("tickets")
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(titles, vec![src.ticket_title.clone()]);
    drop(conn);

    assert_eq!(identity(&pool, 1), before, "workspace 1 keeps its identity");
    assert_eq!(
        count_in(&pool, "workflow_states", 1),
        count_in(&pool, "workflow_states", src.seed.workspace_id),
        "the archive's workflow replaces the seeded one"
    );
    assert_eq!(
        app_name(&pool, 1),
        SOURCE_APP_NAME,
        "self-hosted takes the archive's name"
    );

    let fenced = fence(&pool, 1);
    assert!(fenced.epoch.is_some() && fenced.resync_floor.is_some());

    // Handover.
    #[derive(QueryableByName)]
    struct Flags {
        #[diesel(sql_type = Bool)]
        webhook_on: bool,
        #[diesel(sql_type = Text)]
        plugin_state: String,
        #[diesel(sql_type = BigInt)]
        secrets: i64,
        #[diesel(sql_type = BigInt)]
        settings: i64,
        #[diesel(sql_type = Bool)]
        email_on: bool,
        #[diesel(sql_type = Bool)]
        sealed_password: bool,
        #[diesel(sql_type = Nullable<Text>)]
        guest_token: Option<String>,
    }
    let flags: Flags = diesel::sql_query(
        "SELECT \
           (SELECT bool_or(enabled) FROM webhooks WHERE workspace_id = 1) AS webhook_on, \
           (SELECT string_agg(state::text, ',') FROM plugins WHERE workspace_id = 1) AS plugin_state, \
           (SELECT count(*) FROM plugin_data WHERE workspace_id = 1 AND is_secret) AS secrets, \
           (SELECT count(*) FROM plugin_data WHERE workspace_id = 1 AND NOT is_secret) AS settings, \
           (SELECT enabled FROM workspace_email_settings WHERE workspace_id = 1) AS email_on, \
           (SELECT encrypted_smtp_password IS NOT NULL FROM workspace_email_settings \
              WHERE workspace_id = 1) AS sealed_password, \
           (SELECT guest_lookup_token::text FROM tickets WHERE workspace_id = 1) AS guest_token",
    )
    .get_result(&mut pool.get().expect("conn"))
    .expect("handover");
    assert!(!flags.webhook_on, "webhooks arrive off");
    assert_eq!(flags.plugin_state, "disabled", "plugins arrive off");
    assert_eq!(
        (flags.secrets, flags.settings),
        (0, 1),
        "plugin secrets stay behind"
    );
    assert!(!flags.email_on, "outbound email arrives off");
    assert!(!flags.sealed_password, "the sealed password stays behind");
    let token = flags.guest_token.expect("a guest lookup token");
    assert_ne!(
        token,
        src.guest_token.to_string(),
        "guest lookup tokens are new"
    );
    let handover = serde_json::to_string(&applied.report.handover).expect("handover");
    for kind in [
        "encrypted_setting_dropped",
        "tokens_regenerated",
        "plugin_secrets_dropped",
        "switched_off",
    ] {
        assert!(handover.contains(kind), "{kind} reported: {handover}");
    }
    // References inside JSON: the rule keeps the imported tag under its new
    // id, loses the one the export doesn't carry, turns draft, and the
    // report says so.
    #[derive(QueryableByName)]
    struct RuleTags {
        #[diesel(sql_type = Text)]
        state: String,
        #[diesel(sql_type = Bool)]
        tagged: bool,
    }
    let rule: RuleTags = diesel::sql_query(
        "SELECT r.state::text AS state, \
                r.actions -> 0 -> 'config' -> 'tag_ids' = jsonb_build_array(t.id) AS tagged \
         FROM rules r JOIN tags t ON t.workspace_id = r.workspace_id AND t.name = 'vip' \
         WHERE r.workspace_id = 1 AND r.name = 'Tag VIPs'",
    )
    .get_result(&mut pool.get().expect("conn"))
    .expect("rule");
    assert_eq!(rule.state, "draft");
    assert!(rule.tagged, "the rule names the imported tag's new id");
    let references = serde_json::to_value(&applied.report.references).expect("references");
    for (column, effect) in [("actions", "cleared"), ("state", "rule_drafted")] {
        assert!(
            references
                .as_array()
                .is_some_and(|lines| lines.iter().any(|l| {
                    l["table"] == "rules" && l["column"] == column && l["effect"] == effect
                })),
            "rules.{column} {effect} reported: {references}"
        );
    }
}

/// On hosted the workspace keeps its portal name (#719) along with its id,
/// uuid, slug, name, plan, seat limit and custom domain, and merges the
/// archive's settings over its own. A reset then takes it back to how
/// provisioning left it, files included, and bumps the epoch again.
#[actix_web::test]
async fn a_hosted_import_keeps_the_workspace_and_a_reset_restores_it() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool).await;
    let target = provision_hosted(&pool);
    let before = identity(&pool, target.id);
    let contents = read_archive(&src.archive, Some(PASSWORD)).expect("read");

    let applied = import_into(&pool, &contents, &hosted_target(&target)).expect("imports");
    assert_eq!(applied.workspace_id, target.id);
    assert_eq!(
        identity(&pool, target.id),
        before,
        "the workspace keeps its identity"
    );
    assert_eq!(
        app_name(&pool, target.id),
        target.name,
        "hosted keeps its portal name"
    );
    assert_eq!(count_in(&pool, "tickets", target.id), 1);
    #[derive(QueryableByName)]
    struct Kept {
        #[diesel(sql_type = Nullable<Text>)]
        primary_color: Option<String>,
        #[diesel(sql_type = Bool)]
        settings_kept: bool,
    }
    let kept: Kept = diesel::sql_query(
        "SELECT s.primary_color::text AS primary_color, w.settings ? 'keep' AS settings_kept \
         FROM site_settings s JOIN workspaces w ON w.id = s.workspace_id WHERE w.id = $1",
    )
    .bind::<Integer, _>(target.id)
    .get_result(&mut pool.get().expect("conn"))
    .expect("settings");
    assert_eq!(
        kept.primary_color.as_deref(),
        Some("#123456"),
        "branding replaced"
    );
    assert!(
        kept.settings_kept,
        "the archive's settings merge over the target's"
    );
    let imported = fence(&pool, target.id);
    assert_eq!(imported, applied.fence);
    assert!(imported.epoch.is_some());

    // A second import refuses: the workspace now holds data.
    match import_into(&pool, &contents, &hosted_target(&target)) {
        Err(ApplyError::NotUntouched { found }) => {
            assert_eq!(found[0].what, "tickets", "{found:?}")
        }
        other => panic!("expected workspace_not_empty, got {other:?}"),
    }

    // Reset.
    let storage = WorkspaceScopedStorage::arc(test_storage(), target.id);
    storage
        .put_file(b"x", "tickets/1/attachments/a.pdf", "application/pdf")
        .await
        .expect("put");
    storage
        .put_file(b"x", "imports/0198/upload.csv", "text/csv")
        .await
        .expect("put");
    let mut conn = pool.get().expect("conn");
    let reset = with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        apply::reset(c, target.id, DeploymentMode::Hosted, Some(target.owner))
    })
    .expect("resets");
    drop(conn);
    assert!(
        reset.cleared.iter().any(|f| f.what == "tickets"),
        "{:?}",
        reset.cleared
    );
    assert_eq!(
        apply::clear_files(test_storage(), target.id)
            .await
            .expect("files"),
        1
    );
    let held = apply::held_files(test_storage(), target.id)
        .await
        .expect("list");
    assert_eq!(untouched(&pool, target.id, &held), Vec::<Found>::new());
    assert!(
        storage
            .file_exists("imports/0198/upload.csv")
            .await
            .expect("exists"),
        "files an export leaves out stay"
    );
    assert_eq!(count_in(&pool, "tickets", target.id), 0);
    assert_eq!(count_in(&pool, "workflow_states", target.id), 7);
    assert_eq!(
        count_in(&pool, "documentation_pages", target.id),
        1,
        "the welcome page"
    );
    assert_eq!(app_name(&pool, target.id), target.name);
    assert_eq!(identity(&pool, target.id), before);
    let after_reset = fence(&pool, target.id);
    assert_eq!(after_reset, reset.fence);
    assert_ne!(after_reset.epoch, imported.epoch, "a reset bumps the epoch");
    assert!(after_reset.resync_floor > imported.resync_floor);

    // And imports again.
    import_into(&pool, &contents, &hosted_target(&target)).expect("imports after a reset");
    assert_eq!(count_in(&pool, "tickets", target.id), 1);
}

/// An import that fails part way leaves the workspace as it was: seeded
/// configuration in place, no rows, no epoch.
#[actix_web::test]
async fn a_failed_import_leaves_the_workspace_untouched() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool).await;
    let target = provision_hosted(&pool);
    let mut contents = read_archive(&src.archive, Some(PASSWORD)).expect("read");
    // A ticket the database refuses, after the clear and the earlier tables.
    for row in contents.tables.get_mut("tickets").expect("tickets") {
        row.as_object_mut()
            .expect("row")
            .insert("title".into(), serde_json::Value::Null);
    }

    match import_into(&pool, &contents, &hosted_target(&target)) {
        Err(ApplyError::Failed(_)) => {}
        other => panic!("expected the load to fail, got {other:?}"),
    }
    assert_eq!(untouched(&pool, target.id, &[]), Vec::<Found>::new());
    assert_eq!(count_in(&pool, "workflow_states", target.id), 7);
    assert_eq!(count_in(&pool, "documentation_pages", target.id), 1);
    assert_eq!(count_in(&pool, "webhooks", target.id), 0);
    assert_eq!(app_name(&pool, target.id), target.name);
    assert_eq!(fence(&pool, target.id), DataFence::default());
}

/// One import or reset of a workspace at a time: a second refuses rather
/// than waits.
#[actix_web::test]
async fn a_running_import_holds_off_another() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool).await;
    let target = provision_hosted(&pool);
    let contents = read_archive(&src.archive, Some(PASSWORD)).expect("read");

    let mut holder = pool.get().expect("conn");
    holder
        .transaction::<_, diesel::result::Error, _>(|c| {
            assert!(backend::repository::workspaces::try_lock_import(
                c, target.id
            )?);
            match import_into(&pool, &contents, &hosted_target(&target)) {
                Err(ApplyError::Busy { workspace_id }) => assert_eq!(workspace_id, target.id),
                other => panic!("expected import_in_progress, got {other:?}"),
            }
            Ok(())
        })
        .expect("holds the lock");
    drop(holder);
    assert_eq!(count_in(&pool, "tickets", target.id), 0);
    import_into(&pool, &contents, &hosted_target(&target)).expect("imports once it's free");
}

/// The ticket ids `q` finds from `workspace_id`.
fn ticket_hits(search: &SearchService, workspace_id: i32, q: &str) -> Vec<String> {
    let query = SearchQuery {
        q: q.to_string(),
        limit: 50,
        types: Some("ticket".to_string()),
        sort: None,
        author: None,
    };
    search
        .search(&query, true, workspace_id as i64)
        .expect("search")
        .results
        .into_iter()
        .map(|r| r.id)
        .collect()
}

/// Once the import commits, the target workspace is reindexed, so its
/// imported tickets are found from it.
#[actix_web::test]
async fn an_applied_imports_tickets_are_searchable() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool).await;
    let target = provision_hosted(&pool);
    let index_dir = tempfile::tempdir().expect("index dir");
    let search =
        std::sync::Arc::new(SearchService::new(index_dir.path(), &pool).expect("init search"));
    search.reload().expect("reload");
    let word = src.ticket_title.rsplit(' ').next().expect("title word");
    let from_source = ticket_hits(&search, src.seed.workspace_id, word);
    assert_eq!(from_source.len(), 1, "{from_source:?}");
    assert!(ticket_hits(&search, target.id, word).is_empty());

    let (status, body) = call_admin_route_with_search(
        &pool,
        &src.seed,
        import_request(
            &src.archive,
            &[
                ("password", PASSWORD),
                ("workspace_id", &target.id.to_string()),
                ("regenerate_uuids", "true"),
            ],
        ),
        Some(search.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // The reindex runs in the background after the response.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while search.is_reindexing_workspace(target.id) {
        assert!(
            std::time::Instant::now() < deadline,
            "reindex still running"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    search.reload().expect("reload");

    let from_target = ticket_hits(&search, target.id, word);
    assert_eq!(from_target.len(), 1, "{from_target:?}");
    assert_ne!(from_target, from_source, "the target's own copy is found");
    assert_eq!(
        ticket_hits(&search, src.seed.workspace_id, word),
        from_source
    );
}
