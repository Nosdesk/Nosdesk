//! The daily sweep deletes uploads a signed-in user staged for a reply that
//! nothing claimed in time: the file through its workspace's storage, then the
//! row. Anything a reply claimed, anything still claimable and anything a
//! guest uploaded stays, and a file that can't be deleted keeps its row for the
//! next run. It runs the way production runs it: on the `nosdesk_app` pool,
//! where a connection starts with no workspace.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text, Uuid as SqlUuid};

use backend::models::{NewComment, NewTicket};
use backend::services::scheduled_jobs::unattached_upload_sweep;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::storage::{create_storage, Storage, StorageConfig, StorageError, StoredFile};

type Conn = backend::db::DbConnection;

fn in_workspace<T>(conn: &mut Conn, ws: i32, f: impl FnOnce(&mut Conn) -> QueryResult<T>) -> T {
    let actor = ActorContext::system("test:unattached_upload_seed").with_workspace(ws);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, f).expect("seed")
}

/// An upload row for `temp/{key}`, `hours` old, on `comment` if given.
fn upload(
    conn: &mut Conn,
    ws: i32,
    key: &str,
    uploader: Option<uuid::Uuid>,
    comment: Option<i32>,
    hours: i32,
) -> i32 {
    #[derive(QueryableByName)]
    struct Id {
        #[diesel(sql_type = Integer)]
        id: i32,
    }
    in_workspace(conn, ws, |c| {
        Ok(diesel::sql_query(
            "INSERT INTO attachments (url, name, uploaded_by, comment_id, created_at) \
             VALUES ($1, $2, $3, $4, now() - make_interval(hours => $5)) RETURNING id",
        )
        .bind::<Text, _>(format!("/uploads/temp/{key}"))
        .bind::<Text, _>(key)
        .bind::<Nullable<SqlUuid>, _>(uploader)
        .bind::<Nullable<Integer>, _>(comment)
        .bind::<Integer, _>(hours)
        .get_result::<Id>(c)?
        .id)
    })
}

/// A reply on a new ticket in `ws`.
fn reply(conn: &mut Conn, ws: i32, author: uuid::Uuid) -> i32 {
    in_workspace(conn, ws, |c| {
        use backend::schema::tickets;
        let state = backend::repository::workflow_states::default_state(c)?;
        let ticket: i32 = diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Printer".into(),
                workflow_state_id: state.id,
                ..Default::default()
            })
            .returning(tickets::id)
            .get_result(c)?;
        Ok(backend::repository::comments::create_comment(
            c,
            NewComment {
                content: "Logs attached".into(),
                ticket_id: ticket,
                user_uuid: author,
                ..Default::default()
            },
            None,
        )?
        .id)
    })
}

fn attachment_exists(conn: &mut Conn, id: i32) -> bool {
    use backend::schema::attachments;
    attachments::table
        .find(id)
        .count()
        .get_result::<i64>(conn)
        .expect("count")
        > 0
}

/// Workspaces of the system-attributed `attachment.deleted` events for
/// attachment `id`. Read on the test superuser connection, which sees every
/// workspace's events.
fn system_delete_events(conn: &mut Conn, id: i32) -> Vec<i32> {
    #[derive(QueryableByName)]
    struct Ws {
        #[diesel(sql_type = Integer)]
        workspace_id: i32,
    }
    diesel::sql_query(
        "SELECT workspace_id FROM sync_actions \
         WHERE event_type = 'attachment.deleted' AND aggregate_id = $1 AND actor_uuid IS NULL",
    )
    .bind::<Text, _>(id.to_string())
    .load::<Ws>(conn)
    .expect("sync events")
    .into_iter()
    .map(|r| r.workspace_id)
    .collect()
}

#[actix_web::test]
async fn an_upload_nobody_claimed_goes_with_its_file() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let seeded = crate::common::seed_two_workspaces(&mut conn);
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    let (staff_a, staff_b) = (seeded.a.admin_uuid, seeded.b.admin_uuid);

    // Workspace A: a day-old staged PDF (and the thumbnail `/upload` rendered
    // beside it) goes. An upload a reply claimed, one still claimable, one
    // just past the claim window but inside the sweep's margin, and a guest's
    // stay.
    let old = upload(&mut conn, a, "old.pdf", Some(staff_a), None, 48);
    let young = upload(&mut conn, a, "young.png", Some(staff_a), None, 2);
    let in_margin = upload(&mut conn, a, "margin.png", Some(staff_a), None, 24);
    let comment = reply(&mut conn, a, staff_a);
    // Its row still says `temp/`, so only the claim keeps it.
    let claimed = upload(
        &mut conn,
        a,
        "claimed.png",
        Some(staff_a),
        Some(comment),
        48,
    );
    let guest = upload(&mut conn, a, "guest.png", None, None, 48);
    // Workspace B: its own day-old upload.
    let old_b = upload(&mut conn, b, "b-old.png", Some(staff_b), None, 48);
    drop(conn);

    let dir = tempfile::tempdir().expect("storage dir");
    let storage = create_storage(StorageConfig::Local {
        base_path: dir.path().to_string_lossy().into_owned(),
    });
    let files = [
        format!("ws/{a}/temp/old.pdf"),
        format!("ws/{a}/temp/old_thumb.webp"),
        format!("ws/{a}/temp/young.png"),
        format!("ws/{a}/temp/margin.png"),
        format!("ws/{a}/temp/claimed.png"),
        format!("ws/{a}/temp/guest.png"),
        format!("ws/{b}/temp/b-old.png"),
        // The same names in the other workspace's prefix: each upload's file
        // is found under its own workspace only.
        format!("ws/{b}/temp/old.pdf"),
        format!("ws/{a}/temp/b-old.png"),
    ];
    for path in &files {
        storage
            .put_file(b"x", path, "application/octet-stream")
            .await
            .expect("store file");
    }

    unattached_upload_sweep(&db.job_pool(), storage.clone())
        .await
        .expect("sweep");

    let mut conn = pool.get().expect("conn");
    assert!(!attachment_exists(&mut conn, old), "old upload removed");
    assert!(
        !attachment_exists(&mut conn, old_b),
        "B's old upload removed"
    );
    assert!(attachment_exists(&mut conn, young), "still claimable");
    assert!(attachment_exists(&mut conn, in_margin), "inside the margin");
    assert!(attachment_exists(&mut conn, claimed), "on a reply");
    assert!(attachment_exists(&mut conn, guest), "guest_residue's");
    assert_eq!(
        system_delete_events(&mut conn, old_b),
        vec![b],
        "the delete is the system job's, in the upload's own workspace"
    );

    let mut expected: Vec<String> = files
        .iter()
        .filter(|p| {
            ![
                format!("ws/{a}/temp/old.pdf"),
                format!("ws/{a}/temp/old_thumb.webp"),
                format!("ws/{b}/temp/b-old.png"),
            ]
            .contains(p)
        })
        .cloned()
        .collect();
    expected.sort();
    let mut left = storage.list_prefix("").await.expect("list");
    left.sort();
    assert_eq!(left, expected);
}

/// A store whose deletes all fail.
struct UndeletableStorage;

#[async_trait]
impl Storage for UndeletableStorage {
    async fn store_file(
        &self,
        _data: &[u8],
        _filename: &str,
        _content_type: &str,
        _folder: &str,
    ) -> Result<StoredFile, StorageError> {
        unreachable!("the sweep only deletes")
    }

    async fn put_file(
        &self,
        _data: &[u8],
        _path: &str,
        _content_type: &str,
    ) -> Result<StoredFile, StorageError> {
        unreachable!("the sweep only deletes")
    }

    async fn get_file(&self, _path: &str) -> Result<Vec<u8>, StorageError> {
        unreachable!("the sweep only deletes")
    }

    async fn delete_file(&self, _path: &str) -> Result<(), StorageError> {
        Err(StorageError::Backend("unreachable".into()))
    }

    async fn file_exists(&self, _path: &str) -> Result<bool, StorageError> {
        unreachable!("the sweep only deletes")
    }

    fn get_public_url(&self, path: &str) -> String {
        path.to_string()
    }

    async fn move_file(&self, _from_path: &str, _to_path: &str) -> Result<(), StorageError> {
        unreachable!("the sweep only deletes")
    }

    async fn list_prefix(&self, _prefix: &str) -> Result<Vec<String>, StorageError> {
        unreachable!("the sweep only deletes")
    }
}

#[actix_web::test]
async fn an_upload_whose_file_cant_be_deleted_keeps_its_row() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    let seeded = crate::common::seed_two_workspaces(&mut conn);
    let ws = seeded.a.workspace_id;
    let old = upload(
        &mut conn,
        ws,
        "stuck.png",
        Some(seeded.a.admin_uuid),
        None,
        48,
    );
    drop(conn);

    unattached_upload_sweep(&db.job_pool(), Arc::new(UndeletableStorage))
        .await
        .expect("a failed delete doesn't fail the run");

    let mut conn = pool.get().expect("conn");
    assert!(
        attachment_exists(&mut conn, old),
        "the row stays for the next run"
    );
}
