use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::models::*;

pub fn init<R: Runtime, C: DeserializeOwned>(
  app: &AppHandle<R>,
  _api: PluginApi<R, C>,
) -> crate::Result<WebAuth<R>> {
  Ok(WebAuth(app.clone()))
}

/// Access to the web-auth APIs.
pub struct WebAuth<R: Runtime>(AppHandle<R>);

impl<R: Runtime> WebAuth<R> {
  pub fn authenticate(&self, _payload: AuthenticateRequest) -> crate::Result<AuthenticateResponse> {
    Err(crate::Error::UnsupportedPlatformError)
  }
}
