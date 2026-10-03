//! Email copies of the workspace logos (see `utils::email_logo`).
//!
//! An upload makes the copy for its logo. [`make_missing`] covers the rest:
//! logos uploaded before copies existed, and ones a restore or import brought
//! in without a copy.

use std::sync::Arc;

use tracing::{info, warn};
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::branding::{legacy_logical_path, owned_logical_path, BRANDING_DIR};
use crate::repository::site_settings::{self, Logo};
use crate::utils::email_logo::{self, EmailLogo, Rendition};
use crate::utils::storage::{Storage, WorkspaceScopedStorage};

/// The filename stem of a logo's email copies.
pub fn copy_stem(logo: Logo) -> &'static str {
    match logo {
        Logo::Main => "email_logo",
        Logo::Light => "email_logo_light",
    }
}

/// Render the email copy of an uploaded logo, off the async runtime.
pub async fn render(source: Vec<u8>) -> Result<Rendition, String> {
    tokio::task::spawn_blocking(move || email_logo::render(&source))
        .await
        .map_err(|e| format!("logo render task failed: {e}"))?
}

/// Save a rendered copy in the workspace's branding folder (`storage` is the
/// workspace-scoped handle). Every copy gets a name of its own and is never
/// overwritten, nor removed when the logo is replaced, so a message already
/// sent, or waiting in the queue, keeps the logo it was written with.
pub async fn save(
    storage: &Arc<dyn Storage>,
    workspace_uuid: Uuid,
    logo: Logo,
    rendition: &Rendition,
) -> Result<EmailLogo, String> {
    let version = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let filename = format!("{}_{version}.png", copy_stem(logo));
    storage
        .put_file(
            &rendition.png,
            &format!("{BRANDING_DIR}/{filename}"),
            "image/png",
        )
        .await
        .map_err(|e| format!("could not save the logo's email copy: {e:?}"))?;
    Ok(rendition.logo(format!(
        "/uploads/{BRANDING_DIR}/{workspace_uuid}/{filename}"
    )))
}

/// The storage path of a stored copy, when it is this workspace's.
pub fn stored_copy_path(copy: &serde_json::Value, workspace_uuid: Uuid) -> Option<String> {
    let url = copy.get("url")?.as_str()?;
    owned_logical_path(url, workspace_uuid)
}

/// Make every missing email copy. Returns how many were made; a logo that
/// fails is logged and tried again next time.
pub async fn make_missing(pool: &Pool, base: Arc<dyn Storage>) -> usize {
    // cross-tenant: one pass over every workspace's branding settings.
    let pending = crate::sync::session::background_run(
        pool,
        "scheduler:email_logo_copies",
        site_settings::logos_without_email_copies,
    );
    let pending = match pending {
        Ok(rows) => rows,
        Err(e) => {
            warn!(error = %e, "Could not list logos without an email copy");
            return 0;
        }
    };

    let mut made = 0;
    for row in pending {
        let scoped = WorkspaceScopedStorage::arc(base.clone(), row.workspace_id);
        for (logo, url) in [
            (Logo::Main, row.logo_url),
            (Logo::Light, row.logo_light_url),
        ] {
            let Some(url) = url else { continue };
            match make_missing_one(
                pool,
                &base,
                &scoped,
                row.workspace_id,
                row.workspace_uuid,
                logo,
                &url,
            )
            .await
            {
                Ok(true) => made += 1,
                Ok(false) => {}
                Err(e) => warn!(
                    workspace_id = row.workspace_id,
                    error = %e,
                    "Could not make a logo's email copy"
                ),
            }
        }
    }
    if made > 0 {
        info!(count = made, "Made email copies of workspace logos");
    }
    made
}

async fn make_missing_one(
    pool: &Pool,
    base: &Arc<dyn Storage>,
    scoped: &Arc<dyn Storage>,
    workspace_id: i32,
    workspace_uuid: Uuid,
    logo: Logo,
    url: &str,
) -> Result<bool, String> {
    // The logo sits under the workspace's prefix, or, from before branding
    // was scoped, in the folder every workspace shared.
    let read = if let Some(path) = owned_logical_path(url, workspace_uuid) {
        scoped.get_file(&path).await
    } else if let Some(path) = legacy_logical_path(url) {
        base.get_file(&path).await
    } else {
        return Err("the logo's URL is not a branding upload".into());
    };
    let source = read.map_err(|e| format!("could not read the logo: {e:?}"))?;

    let copy = save(scoped, workspace_uuid, logo, &render(source).await?).await?;
    let copy_path = owned_logical_path(&copy.url, workspace_uuid);
    let value = serde_json::to_value(&copy).map_err(|e| e.to_string())?;
    let source_url = url.to_string();
    let recorded = crate::sync::session::run_in_workspace(
        pool,
        "scheduler:email_logo_copies",
        workspace_id,
        |conn| site_settings::record_email_copy(conn, logo, &source_url, value),
    )
    .map_err(|e| e.to_string());

    // Unrecorded, nothing refers to the copy: a new upload made its own
    // meanwhile, or the write failed and the next run tries again.
    if !matches!(recorded, Ok(true)) {
        if let Some(path) = copy_path {
            let _ = scoped.delete_file(&path).await;
        }
    }
    recorded
}
