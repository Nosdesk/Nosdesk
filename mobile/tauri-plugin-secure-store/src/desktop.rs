use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::models::*;

pub fn init<R: Runtime, C: DeserializeOwned>(
  app: &AppHandle<R>,
  _api: PluginApi<R, C>,
) -> crate::Result<SecureStore<R>> {
  Ok(SecureStore(app.clone()))
}

/// Access to the secure-store APIs.
///
/// Desktop (`tauri dev` preview on macOS/Linux/Windows) has no OS secure-store
/// wiring: these are no-ops, so the preview runs but does not persist the
/// session across restarts. The real device targets (iOS/Android) use the
/// native plugin code in `ios/` and `android/`.
pub struct SecureStore<R: Runtime>(#[allow(dead_code)] AppHandle<R>);

impl<R: Runtime> SecureStore<R> {
  pub fn save(&self, _payload: SaveRequest) -> crate::Result<()> {
    Ok(())
  }

  pub fn load(&self) -> crate::Result<LoadResponse> {
    Ok(LoadResponse::default())
  }

  pub fn clear(&self) -> crate::Result<()> {
    Ok(())
  }
}
