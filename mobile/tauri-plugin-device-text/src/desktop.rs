use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::models::*;

pub fn init<R: Runtime, C: DeserializeOwned>(
  app: &AppHandle<R>,
  _api: PluginApi<R, C>,
) -> crate::Result<DeviceText<R>> {
  Ok(DeviceText(app.clone()))
}

/// Access to the device text APIs.
///
/// Desktop (`tauri dev` preview) has no system text size to follow: the
/// webview's own zoom and font settings already apply, so this reports the
/// default and never changes.
pub struct DeviceText<R: Runtime>(#[allow(dead_code)] AppHandle<R>);

impl<R: Runtime> DeviceText<R> {
  pub fn get(&self) -> crate::Result<TextSettings> {
    Ok(TextSettings::default())
  }
}
