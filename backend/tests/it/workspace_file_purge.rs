//! A hard-deleted workspace's stored files are removed by the purge job. The
//! hard delete queues them in its own transaction; the job deletes everything
//! under `ws/{id}/` and records completion; a storage failure leaves the purge
//! queued for the next run.

use async_trait::async_trait;
use chrono::Utc;
use diesel::prelude::*;

use backend::repository::{workspace_file_purges, workspaces};
use backend::schema::workspace_file_purges as purges;
use backend::services::scheduled_jobs::purge_deleted_workspace_files_in;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_bypass_context;
use backend::utils::storage::{create_storage, Storage, StorageConfig, StorageError, StoredFile};

use crate::common::{self, TestPool};

/// Run `f` the way the scheduler does: elevated, since the workspace and its
/// purge row are platform-level.
fn as_admin<T>(
    pool: &TestPool,
    f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>,
) -> T {
    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system("test:file_purge"), f)
        .expect("elevated query")
}

#[actix_web::test]
async fn a_hard_deleted_workspaces_files_are_purged() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (gone, kept) = (seeded.a.workspace_id, seeded.b.workspace_id);

    let dir = tempfile::tempdir().expect("storage dir");
    let storage = create_storage(StorageConfig::Local {
        base_path: dir.path().to_string_lossy().into_owned(),
    });
    for path in [
        format!("ws/{gone}/tickets/1/scan.pdf"),
        format!("ws/{gone}/temp/draft.png"),
        format!("ws/{kept}/tickets/2/invoice.pdf"),
    ] {
        storage
            .put_file(b"x", &path, "application/octet-stream")
            .await
            .expect("store file");
    }

    // Archive and hard-delete A, as the daily sweep does past the grace window.
    let cutoff = Utc::now() + chrono::Duration::hours(1);
    let deleted = as_admin(&pool, |c| {
        workspaces::archive_workspace(c, gone)?;
        workspaces::hard_delete_workspace(c, gone, cutoff)
    });
    assert_eq!(deleted, 1);
    assert_eq!(as_admin(&pool, workspace_file_purges::pending), vec![gone]);

    purge_deleted_workspace_files_in(&pool, storage.as_ref())
        .await
        .expect("purge run");

    assert_eq!(
        storage.list_prefix("").await.expect("list"),
        vec![format!("ws/{kept}/tickets/2/invoice.pdf")],
        "only the deleted workspace's files are removed"
    );
    assert!(
        as_admin(&pool, workspace_file_purges::pending).is_empty(),
        "the purge is recorded complete"
    );
}

/// A store that can't be listed, so every purge fails.
struct UnreachableStorage;

#[async_trait]
impl Storage for UnreachableStorage {
    async fn store_file(
        &self,
        _data: &[u8],
        _filename: &str,
        _content_type: &str,
        _folder: &str,
    ) -> Result<StoredFile, StorageError> {
        unreachable!("the purge only lists and deletes")
    }

    async fn put_file(
        &self,
        _data: &[u8],
        _path: &str,
        _content_type: &str,
    ) -> Result<StoredFile, StorageError> {
        unreachable!("the purge only lists and deletes")
    }

    async fn get_file(&self, _path: &str) -> Result<Vec<u8>, StorageError> {
        unreachable!("the purge only lists and deletes")
    }

    async fn delete_file(&self, _path: &str) -> Result<(), StorageError> {
        Err(StorageError::Backend("unreachable".into()))
    }

    async fn file_exists(&self, _path: &str) -> Result<bool, StorageError> {
        unreachable!("the purge only lists and deletes")
    }

    fn get_public_url(&self, path: &str) -> String {
        path.to_string()
    }

    async fn move_file(&self, _from_path: &str, _to_path: &str) -> Result<(), StorageError> {
        unreachable!("the purge only lists and deletes")
    }

    async fn list_prefix(&self, _prefix: &str) -> Result<Vec<String>, StorageError> {
        Err(StorageError::Backend("unreachable".into()))
    }
}

#[actix_web::test]
async fn a_failed_purge_stays_queued() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let workspace_id = 424_242;
    as_admin(&pool, |c| workspace_file_purges::queue(c, workspace_id));

    purge_deleted_workspace_files_in(&pool, &UnreachableStorage)
        .await
        .expect("a failed purge doesn't fail the run");

    assert_eq!(
        as_admin(&pool, workspace_file_purges::pending),
        vec![workspace_id]
    );
    let (attempts, last_error): (i32, Option<String>) = as_admin(&pool, |c| {
        purges::table
            .find(workspace_id)
            .select((purges::attempts, purges::last_error))
            .first(c)
    });
    assert_eq!(attempts, 1);
    assert_eq!(last_error.as_deref(), Some("storage delete failed"));
}
