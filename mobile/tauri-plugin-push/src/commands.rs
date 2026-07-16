use tauri::{command, AppHandle, Runtime};

use crate::models::*;
use crate::PushExt;
use crate::Result;

#[command]
pub(crate) async fn request_permission<R: Runtime>(
  app: AppHandle<R>,
) -> Result<PermissionResponse> {
  app.push().request_permission()
}

#[command]
pub(crate) async fn get_token<R: Runtime>(app: AppHandle<R>) -> Result<TokenResponse> {
  app.push().get_token()
}

#[command]
pub(crate) async fn get_pending_notification<R: Runtime>(
  app: AppHandle<R>,
) -> Result<PendingNotification> {
  app.push().get_pending_notification()
}
