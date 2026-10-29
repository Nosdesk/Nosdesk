//! Native share sheet for a file the app has already written to disk.
//!
//! The webview can't save, send or print a file itself: `<a download>` does
//! nothing useful in WKWebView / Android WebView and `window.print()` is a
//! no-op. This plugin hands the file to the OS instead: `UIActivityViewController`
//! on iOS (`ios/`, which offers Save to Files, AirDrop, Mail and Print) and an
//! `ACTION_SEND` chooser through a `FileProvider` on Android (`android/`).
//! Desktop (`tauri dev` preview) has no share sheet and returns an error.

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
use desktop::Share;
#[cfg(mobile)]
use mobile::Share;

/// Extensions to access the share APIs from a [`tauri::Manager`].
pub trait ShareExt<R: Runtime> {
    fn share(&self) -> &Share<R>;
}

impl<R: Runtime, T: Manager<R>> crate::ShareExt<R> for T {
    fn share(&self) -> &Share<R> {
        self.state::<Share<R>>().inner()
    }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("share")
        .invoke_handler(tauri::generate_handler![commands::share_file])
        .setup(|app, api| {
            #[cfg(mobile)]
            let share = mobile::init(app, api)?;
            #[cfg(desktop)]
            let share = desktop::init(app, api)?;
            app.manage(share);
            Ok(())
        })
        .build()
}
