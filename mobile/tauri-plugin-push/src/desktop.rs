use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::models::*;

pub fn init<R: Runtime, C: DeserializeOwned>(
  app: &AppHandle<R>,
  _api: PluginApi<R, C>,
) -> crate::Result<Push<R>> {
  Ok(Push(app.clone()))
}

/// Access to the push APIs.
///
/// Desktop (`tauri dev` preview) has no APNs/FCM: these are no-ops so the
/// preview runs but never obtains a push token. The real device targets
/// (iOS/Android) use the native plugin code in `ios/` and `android/`.
pub struct Push<R: Runtime>(#[allow(dead_code)] AppHandle<R>);

impl<R: Runtime> Push<R> {
  pub fn request_permission(&self) -> crate::Result<PermissionResponse> {
    Ok(PermissionResponse::default())
  }

  pub fn get_token(&self) -> crate::Result<TokenResponse> {
    Ok(TokenResponse::default())
  }

  pub fn get_pending_notification(&self) -> crate::Result<PendingNotification> {
    Ok(PendingNotification::default())
  }
}
