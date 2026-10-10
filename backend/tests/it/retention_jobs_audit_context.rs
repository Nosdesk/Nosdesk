//! Retention jobs that delete rows from audited tables record each delete as
//! the system job, in the workspace the row belongs to. They run the way
//! production runs them: on the `nosdesk_app` pool, where a connection starts
//! with no workspace, so each delete has to bring its own.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use diesel::prelude::*;
use diesel::sql_types::{Integer, Text, Uuid as SqlUuid};

use backend::models::NewWebhookDelivery;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

fn system_delete_audits(conn: &mut backend::db::DbConnection, table: &str, pk: &str) -> Vec<i32> {
    #[derive(QueryableByName)]
    struct Ws {
        #[diesel(sql_type = Integer)]
        workspace_id: i32,
    }
    diesel::sql_query(
        "SELECT workspace_id FROM audit_log \
         WHERE table_name = $1 AND pk_text = $2 AND op = 'D' AND actor_uuid IS NULL",
    )
    .bind::<Text, _>(table)
    .bind::<Text, _>(pk)
    .load::<Ws>(conn)
    .expect("audit rows")
    .into_iter()
    .map(|r| r.workspace_id)
    .collect()
}

#[actix_web::test]
async fn a_soft_deleted_user_past_the_grace_window_is_purged_and_audited() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let seeded = crate::common::seed_two_workspaces(&mut conn);
    let ws = seeded.b.workspace_id;
    let gone = seeded.b.member_uuid;
    let restorable = seeded.a.member_uuid;
    // In no workspace: nowhere to record its purge, so it stays (and the job
    // says so once per run).
    let homeless = crate::common::insert_plain_user(&mut conn, "Homeless");

    for uuid in [gone, homeless] {
        diesel::sql_query(
            "UPDATE users SET deleted_at = now() - interval '400 days' WHERE uuid = $1",
        )
        .bind::<SqlUuid, _>(uuid)
        .execute(&mut conn)
        .expect("soft-delete long ago");
    }
    diesel::sql_query("UPDATE users SET deleted_at = now() WHERE uuid = $1")
        .bind::<SqlUuid, _>(restorable)
        .execute(&mut conn)
        .expect("soft-delete just now");

    let tmp = tempfile::tempdir().expect("temp search dir");
    let search =
        Arc::new(backend::services::search::SearchService::new(tmp.path(), &pool).expect("search"));
    backend::services::scheduled_jobs::purge_soft_deleted_users(db.job_pool(), search)
        .await
        .expect("purge");

    let remaining = |conn: &mut backend::db::DbConnection, uuid: uuid::Uuid| -> i64 {
        backend::schema::users::table
            .find(uuid)
            .count()
            .get_result(conn)
            .expect("count")
    };
    assert_eq!(
        remaining(&mut conn, gone),
        0,
        "purged after the grace window"
    );
    assert_eq!(
        system_delete_audits(&mut conn, "users", &gone.to_string()),
        vec![ws],
        "the purge is audited as the system job, in the user's workspace"
    );
    assert_eq!(
        remaining(&mut conn, restorable),
        1,
        "still within its window"
    );
    assert_eq!(
        remaining(&mut conn, homeless),
        1,
        "no workspace to record it in"
    );
}

#[actix_web::test]
async fn webhook_deliveries_past_retention_are_pruned_and_audited() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let seeded = crate::common::seed_two_workspaces(&mut conn);

    // One old and one fresh delivery in each workspace.
    let mut seed = |ws: i32, webhook_id: i32, age_days: i32| -> i32 {
        let actor = ActorContext::system("test:webhook_delivery_seed").with_workspace(ws);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            let d = backend::repository::webhooks::create_delivery(
                c,
                NewWebhookDelivery {
                    webhook_id,
                    event_type: crate::common::FIXTURE_WEBHOOK_EVENT.to_string(),
                    payload: serde_json::json!({ "id": 1 }),
                    request_headers: None,
                    attempt_number: 1,
                    next_retry_at: None,
                },
            )?;
            diesel::sql_query(
                "UPDATE webhook_deliveries SET created_at = now() - make_interval(days => $1) \
                 WHERE id = $2",
            )
            .bind::<Integer, _>(age_days)
            .bind::<Integer, _>(d.id)
            .execute(c)?;
            Ok(d.id)
        })
        .expect("seed delivery")
    };
    let old_a = seed(seeded.a.workspace_id, seeded.a.webhook_id, 45);
    let new_a = seed(seeded.a.workspace_id, seeded.a.webhook_id, 1);
    let old_b = seed(seeded.b.workspace_id, seeded.b.webhook_id, 45);
    let new_b = seed(seeded.b.workspace_id, seeded.b.webhook_id, 1);

    backend::services::scheduled_jobs::prune_webhook_deliveries(db.job_pool())
        .await
        .expect("prune");

    let exists = |conn: &mut backend::db::DbConnection, id: i32| -> bool {
        backend::schema::webhook_deliveries::table
            .find(id)
            .count()
            .get_result::<i64>(conn)
            .expect("count")
            > 0
    };
    assert!(
        !exists(&mut conn, old_a) && !exists(&mut conn, old_b),
        "old deliveries pruned"
    );
    assert!(
        exists(&mut conn, new_a) && exists(&mut conn, new_b),
        "recent deliveries kept"
    );
    assert_eq!(
        system_delete_audits(&mut conn, "webhook_deliveries", &old_a.to_string()),
        vec![seeded.a.workspace_id]
    );
    assert_eq!(
        system_delete_audits(&mut conn, "webhook_deliveries", &old_b.to_string()),
        vec![seeded.b.workspace_id]
    );
}
