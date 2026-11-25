use tauri::{command, AppHandle, Runtime};

use crate::models::*;
use crate::DeviceTextExt;
use crate::Result;

#[command]
pub(crate) async fn get<R: Runtime>(app: AppHandle<R>) -> Result<TextSettings> {
  app.device_text().get()
}
