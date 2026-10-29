use tauri::{command, AppHandle, Runtime};

use crate::models::*;
use crate::Result;
use crate::ShareExt;

/// Opens the share sheet for `payload.path` and resolves once it closes.
#[command]
pub(crate) async fn share_file<R: Runtime>(
    app: AppHandle<R>,
    payload: ShareFileRequest,
) -> Result<ShareFileResponse> {
    app.share().share_file(payload).await
}
