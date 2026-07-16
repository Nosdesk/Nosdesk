use serde::de::DeserializeOwned;
use tauri::{
  plugin::{PluginApi, PluginHandle},
  AppHandle, Runtime,
};

use crate::models::*;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_push);

/// Registers the Kotlin (Android) / Swift (iOS) plugin class.
pub fn init<R: Runtime, C: DeserializeOwned>(
  _app: &AppHandle<R>,
  api: PluginApi<R, C>,
) -> crate::Result<Push<R>> {
  #[cfg(target_os = "android")]
  let handle = api.register_android_plugin("com.nosdesk.plugin.push", "PushPlugin")?;
  #[cfg(target_os = "ios")]
  let handle = api.register_ios_plugin(init_plugin_push)?;
  Ok(Push(handle))
}

/// Access to the push APIs.
pub struct Push<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Push<R> {
  pub fn request_permission(&self) -> crate::Result<PermissionResponse> {
    self
      .0
      .run_mobile_plugin("requestPermission", ())
      .map_err(Into::into)
  }

  pub fn get_token(&self) -> crate::Result<TokenResponse> {
    self
      .0
      .run_mobile_plugin("getToken", ())
      .map_err(Into::into)
  }
}
