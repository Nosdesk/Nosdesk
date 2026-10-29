use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::models::*;

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<Share<R>> {
    Ok(Share(app.clone()))
}

/// Access to the share APIs.
///
/// Desktop (`tauri dev` preview on macOS/Linux/Windows) has no share sheet
/// wiring, so `share_file` returns an error. The real device targets
/// (iOS/Android) use the native plugin code in `ios/` and `android/`.
pub struct Share<R: Runtime>(#[allow(dead_code)] AppHandle<R>);

impl<R: Runtime> Share<R> {
    pub async fn share_file(&self, _payload: ShareFileRequest) -> crate::Result<ShareFileResponse> {
        Err(crate::Error::Unsupported)
    }
}
