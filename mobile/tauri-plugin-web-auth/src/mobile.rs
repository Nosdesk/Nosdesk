use serde::de::DeserializeOwned;
use tauri::{
  plugin::{PluginApi, PluginHandle},
  AppHandle, Runtime,
};

use crate::models::*;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_web_auth);

// initializes the Kotlin or Swift plugin classes
pub fn init<R: Runtime, C: DeserializeOwned>(
  _app: &AppHandle<R>,
  api: PluginApi<R, C>,
) -> crate::Result<WebAuth<R>> {
  #[cfg(target_os = "android")]
  let handle = api.register_android_plugin("ch.manaf.tauri_plugins.web_auth", "WebAuthPlugin")?;
  #[cfg(target_os = "ios")]
  let handle = api.register_ios_plugin(init_plugin_web_auth)?;
  Ok(WebAuth(handle))
}

/// Access to the web-auth APIs.
pub struct WebAuth<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> WebAuth<R> {
  pub fn authenticate(&self, payload: AuthenticateRequest) -> crate::Result<AuthenticateResponse> {
    self
      .0
      .run_mobile_plugin("authenticate", payload)
      .map_err(Into::into)
  }
}
