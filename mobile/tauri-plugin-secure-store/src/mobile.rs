use serde::de::DeserializeOwned;
use tauri::{
  plugin::{PluginApi, PluginHandle},
  AppHandle, Runtime,
};

use crate::models::*;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_secure_store);

/// Registers the Kotlin (Android) / Swift (iOS) plugin class.
pub fn init<R: Runtime, C: DeserializeOwned>(
  _app: &AppHandle<R>,
  api: PluginApi<R, C>,
) -> crate::Result<SecureStore<R>> {
  #[cfg(target_os = "android")]
  let handle =
    api.register_android_plugin("com.nosdesk.plugin.securestore", "SecureStorePlugin")?;
  #[cfg(target_os = "ios")]
  let handle = api.register_ios_plugin(init_plugin_secure_store)?;
  Ok(SecureStore(handle))
}

/// Access to the secure-store APIs.
pub struct SecureStore<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> SecureStore<R> {
  pub fn save(&self, payload: SaveRequest) -> crate::Result<()> {
    self.0.run_mobile_plugin("save", payload).map_err(Into::into)
  }

  pub fn load(&self) -> crate::Result<LoadResponse> {
    self.0.run_mobile_plugin("load", ()).map_err(Into::into)
  }

  pub fn clear(&self) -> crate::Result<()> {
    self.0.run_mobile_plugin("clear", ()).map_err(Into::into)
  }
}
