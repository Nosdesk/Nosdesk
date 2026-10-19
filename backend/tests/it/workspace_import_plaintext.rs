//! A workspace exported without a password imports. The export leaves the
//! archive policy's secrets out, and the import applies its rules: the webhook
//! comes back off with a new secret and default headers, a person whose MFA
//! secret was left out arrives without MFA, and a ticket row that leaves out a
//! defaulted column takes the default.

use diesel::prelude::*;
use diesel::sql_types::{Bool, Integer, Nullable, SmallInt, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::services::workspace_export::export_workspace;
use backend::services::workspace_import::{import_workspace, read_archive, ImportOptions};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_bypass_context;

use crate::common::{insert_plain_user, seed_two_workspaces, TestDb};

const REF: &str = "test:workspace_import_plaintext";

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

#[derive(QueryableByName)]
struct Webhook {
    #[diesel(sql_type = Text)]
    secret: String,
    #[diesel(sql_type = Bool)]
    enabled: bool,
    #[diesel(sql_type = Nullable<Text>)]
    headers: Option<String>,
}

#[derive(QueryableByName)]
struct Mfa {
    #[diesel(sql_type = Bool)]
    mfa_enabled: bool,
    #[diesel(sql_type = Nullable<SmallInt>)]
    mfa_secret_kek_id: Option<i16>,
}

#[derive(QueryableByName)]
struct Ticket {
    #[diesel(sql_type = Bool)]
    raised_by_staff: bool,
}

fn as_diesel<T>(
    r: Result<T, backend::services::backup::BackupError>,
) -> Result<T, diesel::result::Error> {
    r.map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
}

fn webhooks_in(conn: &mut PgConnection, workspace_id: i32) -> Vec<Webhook> {
    diesel::sql_query(
        "SELECT secret, enabled, headers::text AS headers FROM webhooks WHERE workspace_id = $1",
    )
    .bind::<Integer, _>(workspace_id)
    .load(conn)
    .expect("webhooks")
}

#[test]
fn a_plaintext_export_with_a_webhook_imports() {
    let db = TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = &seeded.a;
    let mut conn = pool.get().expect("conn");

    // A ticket staff raised, so the column's default differs from the source.
    let state: Id = diesel::sql_query(
        "SELECT id FROM workflow_states WHERE workspace_id = $1 ORDER BY id LIMIT 1",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .get_result(&mut conn)
    .expect("a workflow state");
    diesel::sql_query(
        "INSERT INTO tickets (workspace_id, title, workflow_state_id, requester_uuid) \
         VALUES ($1, 'Printer jammed', $2, $3)",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .bind::<Integer, _>(state.id)
    .bind::<SqlUuid, _>(ws.admin_uuid)
    .execute(&mut conn)
    .expect("ticket");
    diesel::sql_query("UPDATE tickets SET raised_by_staff = true WHERE workspace_id = $1")
        .bind::<Integer, _>(ws.workspace_id)
        .execute(&mut conn)
        .expect("raised by staff");

    // A member with MFA on.
    let person = insert_plain_user(&mut conn, "Has MFA");
    diesel::sql_query(
        "UPDATE users SET mfa_enabled = true, mfa_secret = '\\x01'::bytea, \
         mfa_secret_kek_id = 1 WHERE uuid = $1",
    )
    .bind::<SqlUuid, _>(person)
    .execute(&mut conn)
    .expect("mfa on");
    diesel::sql_query(
        "INSERT INTO workspace_members (workspace_id, user_uuid, role) VALUES ($1, $2, 'member')",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .bind::<SqlUuid, _>(person)
    .execute(&mut conn)
    .expect("membership");

    let before = webhooks_in(&mut conn, ws.workspace_id);
    assert_eq!(before.len(), 1, "the fixture webhook");
    assert!(before[0].enabled);

    let actor = ActorContext::system(REF);
    let archive = with_actor_bypass_context(&mut conn, &actor, |c| {
        as_diesel(export_workspace(c, ws.workspace_id, None))
    })
    .expect("plaintext export");
    let mut contents = read_archive(&archive, None).expect("read archive");

    // The export left the secrets out.
    let webhooks = &contents.tables["webhooks"];
    assert_eq!(webhooks.len(), 1);
    assert!(webhooks[0].get("secret").is_none() && webhooks[0].get("headers").is_none());
    let person_row = contents.tables["users"]
        .iter()
        .find(|u| u["uuid"] == person.to_string())
        .expect("the member is exported");
    assert!(person_row.get("mfa_secret").is_none());
    assert_eq!(person_row["mfa_enabled"], true);

    // A ticket row without `raised_by_staff`, as an archive written before the
    // column existed carries once migrated, or one written by another tool.
    let tickets = contents.tables.get_mut("tickets").expect("tickets");
    assert_eq!(tickets.len(), 1);
    for ticket in tickets.iter_mut() {
        ticket
            .as_object_mut()
            .expect("ticket row")
            .remove("raised_by_staff");
    }

    // The person isn't on the target yet, so the import creates them.
    diesel::sql_query("DELETE FROM workspace_members WHERE user_uuid = $1")
        .bind::<SqlUuid, _>(person)
        .execute(&mut conn)
        .expect("remove membership");
    diesel::sql_query("DELETE FROM users WHERE uuid = $1")
        .bind::<SqlUuid, _>(person)
        .execute(&mut conn)
        .expect("remove person");

    let opts = ImportOptions {
        slug_override: Some(format!("plain-{}", Uuid::now_v7().simple())),
        uuid_override: None,
        regenerate_uuids: true,
    };
    let result = with_actor_bypass_context(&mut conn, &actor, |c| {
        as_diesel(import_workspace(c, &contents, &opts))
    })
    .expect("the plaintext archive imports");
    assert_ne!(result.workspace_id, ws.workspace_id);

    let after = webhooks_in(&mut conn, result.workspace_id);
    assert_eq!(after.len(), 1);
    assert!(
        !after[0].enabled,
        "off until the receiver has the new secret"
    );
    assert!(after[0].secret.starts_with("whsec_") && after[0].secret.len() == 70);
    assert_ne!(after[0].secret, before[0].secret);
    assert_eq!(
        after[0].headers.as_deref(),
        Some("{}"),
        "headers take the default"
    );

    let mfa: Mfa =
        diesel::sql_query("SELECT mfa_enabled, mfa_secret_kek_id FROM users WHERE uuid = $1")
            .bind::<SqlUuid, _>(person)
            .get_result(&mut conn)
            .expect("the person is imported");
    assert!(!mfa.mfa_enabled);
    assert_eq!(mfa.mfa_secret_kek_id, None);

    let ticket: Ticket =
        diesel::sql_query("SELECT raised_by_staff FROM tickets WHERE workspace_id = $1")
            .bind::<Integer, _>(result.workspace_id)
            .get_result(&mut conn)
            .expect("the ticket is imported");
    assert!(!ticket.raised_by_staff, "the column takes its default");
}
