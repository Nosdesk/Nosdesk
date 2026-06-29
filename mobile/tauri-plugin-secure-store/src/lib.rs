//! Secure storage for the mobile app's auth refresh token.
//!
//! The token is a credential and must live in the OS secure store, never in the
//! webview's `localStorage`. This plugin keeps the native implementations side
//! by side: the iOS Keychain (`ios/`) and the Android Keystore via
//! `EncryptedSharedPreferences` (`android/`). Desktop (`tauri dev` preview) is a
//! no-op (no persistence), since the device targets are iOS and Android.

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
use desktop::SecureStore;
#[cfg(mobile)]
use mobile::SecureStore;

/// Extensions to access the secure-store APIs from a [`tauri::Manager`].
pub trait SecureStoreExt<R: Runtime> {
  fn secure_store(&self) -> &SecureStore<R>;
}

impl<R: Runtime, T: Manager<R>> crate::SecureStoreExt<R> for T {
  fn secure_store(&self) -> &SecureStore<R> {
    self.state::<SecureStore<R>>().inner()
  }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("secure-store")
    .invoke_handler(tauri::generate_handler![
      commands::save,
      commands::load,
      commands::clear
    ])
    .setup(|app, api| {
      #[cfg(mobile)]
      let secure_store = mobile::init(app, api)?;
      #[cfg(desktop)]
      let secure_store = desktop::init(app, api)?;
      app.manage(secure_store);
      Ok(())
    })
    .build()
}
