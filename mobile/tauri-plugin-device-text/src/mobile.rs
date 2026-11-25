use serde::{de::DeserializeOwned, Serialize};
use tauri::{
  ipc::{Channel, InvokeResponseBody},
  plugin::{PluginApi, PluginHandle},
  AppHandle, Emitter, Runtime,
};

use crate::models::*;
use crate::CHANGE_EVENT;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_device_text);

#[derive(Serialize)]
struct EventHandler {
  handler: Channel,
}

/// Registers the Kotlin (Android) / Swift (iOS) plugin class and gives it a
/// channel for changes. The native side sends on it whenever the user changes
/// text size or Bold Text; each value is bounded here and re-emitted to the
/// webview as [`CHANGE_EVENT`].
pub fn init<R: Runtime, C: DeserializeOwned>(
  app: &AppHandle<R>,
  api: PluginApi<R, C>,
) -> crate::Result<DeviceText<R>> {
  #[cfg(target_os = "android")]
  let handle = api.register_android_plugin("com.nosdesk.plugin.devicetext", "DeviceTextPlugin")?;
  #[cfg(target_os = "ios")]
  let handle = api.register_ios_plugin(init_plugin_device_text)?;

  let app_handle = app.clone();
  let handler = Channel::new(move |body| {
    if let InvokeResponseBody::Json(json) = body {
      if let Ok(settings) = serde_json::from_str::<TextSettings>(&json) {
        let _ = app_handle.emit(CHANGE_EVENT, settings.sanitized());
      }
    }
    Ok(())
  });
  // Without live changes the webview still reads the settings at launch and
  // whenever the app returns to the foreground, so a failure here must not
  // stop the app from starting.
  let _ = handle.run_mobile_plugin::<()>("setEventHandler", EventHandler { handler });

  Ok(DeviceText(handle))
}

/// Access to the device text APIs.
pub struct DeviceText<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> DeviceText<R> {
  pub fn get(&self) -> crate::Result<TextSettings> {
    self
      .0
      .run_mobile_plugin::<TextSettings>("get", ())
      .map(TextSettings::sanitized)
      .map_err(Into::into)
  }
}
