//! Follows the phone's text size and Bold Text settings in the webview.
//!
//! Neither webview does this well on its own. WKWebView ignores Dynamic Type
//! entirely. Android's WebView multiplies every font size by the system font
//! scale (`textZoom`), which grows text inside boxes sized for the default and
//! would double any scale the app applies itself; once the web layer follows
//! the reported scale, the Android side turns that off, so one path scales text
//! everywhere.
//!
//! Two ways in for the web layer (`mobile/src/deviceText.ts`):
//! - `get`: the current settings, read at launch and on returning to the
//!   foreground.
//! - the [`CHANGE_EVENT`] event, sent when the user changes either setting
//!   while the app is open (Control Center's Text Size control, a split
//!   screen Settings app, Android quick settings).
//! - `follow`: the web layer applies the scale itself from now on. Android
//!   then stops its WebView scaling text too; until it is called, Android keeps
//!   the WebView's own scaling. iOS has nothing to stop.
//!
//! Desktop reports the default and never changes.

use tauri::{
  plugin::{Builder, TauriPlugin},
  Manager, Runtime,
};

pub use models::*;

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

mod commands;
mod error;
mod models;

pub use error::{Error, Result};

#[cfg(desktop)]
use desktop::DeviceText;
#[cfg(mobile)]
use mobile::DeviceText;

/// The event a settings change reaches the webview on, with a
/// [`TextSettings`] payload.
pub const CHANGE_EVENT: &str = "device-text://change";

/// Extensions to access the device text APIs from a [`tauri::Manager`].
pub trait DeviceTextExt<R: Runtime> {
  fn device_text(&self) -> &DeviceText<R>;
}

impl<R: Runtime, T: Manager<R>> crate::DeviceTextExt<R> for T {
  fn device_text(&self) -> &DeviceText<R> {
    self.state::<DeviceText<R>>().inner()
  }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("device-text")
    .invoke_handler(tauri::generate_handler![commands::get, commands::follow])
    .setup(|app, api| {
      #[cfg(mobile)]
      let device_text = mobile::init(app, api)?;
      #[cfg(desktop)]
      let device_text = desktop::init(app, api)?;
      app.manage(device_text);
      Ok(())
    })
    .build()
}
