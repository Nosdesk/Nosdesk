use serde::de::DeserializeOwned;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::models::*;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_share);

/// Registers the Kotlin (Android) / Swift (iOS) plugin class.
pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<Share<R>> {
    #[cfg(target_os = "android")]
    let handle = api.register_android_plugin("com.nosdesk.plugin.share", "SharePlugin")?;
    #[cfg(target_os = "ios")]
    let handle = api.register_ios_plugin(init_plugin_share)?;
    Ok(Share(handle))
}

/// Access to the share APIs.
pub struct Share<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Share<R> {
    /// Async so the sheet can stay open for as long as the user takes without
    /// parking a runtime thread.
    pub async fn share_file(&self, payload: ShareFileRequest) -> crate::Result<ShareFileResponse> {
        self.0
            .run_mobile_plugin_async("shareFile", payload)
            .await
            .map_err(Into::into)
    }
}
