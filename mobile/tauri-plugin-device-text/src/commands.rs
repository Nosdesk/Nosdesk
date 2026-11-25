use tauri::{command, AppHandle, Runtime};

use crate::models::*;
use crate::DeviceTextExt;
use crate::Result;

#[command]
pub(crate) async fn get<R: Runtime>(app: AppHandle<R>) -> Result<TextSettings> {
  app.device_text().get()
}

#[command]
pub(crate) async fn follow<R: Runtime>(app: AppHandle<R>) -> Result<()> {
  app.device_text().follow()
}
