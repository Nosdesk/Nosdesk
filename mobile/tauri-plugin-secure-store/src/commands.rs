use tauri::{command, AppHandle, Runtime};

use crate::models::*;
use crate::Result;
use crate::SecureStoreExt;

#[command]
pub(crate) async fn save<R: Runtime>(app: AppHandle<R>, payload: SaveRequest) -> Result<()> {
  app.secure_store().save(payload)
}

#[command]
pub(crate) async fn load<R: Runtime>(app: AppHandle<R>) -> Result<LoadResponse> {
  app.secure_store().load()
}

#[command]
pub(crate) async fn clear<R: Runtime>(app: AppHandle<R>) -> Result<()> {
  app.secure_store().clear()
}
