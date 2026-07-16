//! Push-notification device-token registration for the mobile app.
//!
//! Thin plugin exposing two commands to the JS layer:
//! - `request_permission` — prompt for notification permission (and, on iOS,
//!   start APNs registration),
//! - `get_token` — return the platform push token (APNs hex on iOS, FCM token
//!   on Android) once available.
//!
//! The JS shell (`mobile/src/push.ts`) calls these after the session is
//! established and POSTs the token to `/api/notifications/devices`. The native
//! implementations live in `ios/` (APNs, via app-delegate swizzling to capture
//! the token) and `android/` (a stub until FCM/`google-services.json` is set
//! up). Desktop preview is a no-op.

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
use desktop::Push;
#[cfg(mobile)]
use mobile::Push;

/// Extensions to access the push APIs from a [`tauri::Manager`].
pub trait PushExt<R: Runtime> {
  fn push(&self) -> &Push<R>;
}

impl<R: Runtime, T: Manager<R>> crate::PushExt<R> for T {
  fn push(&self) -> &Push<R> {
    self.state::<Push<R>>().inner()
  }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("push")
    .invoke_handler(tauri::generate_handler![
      commands::request_permission,
      commands::get_token,
      commands::get_pending_notification
    ])
    .setup(|app, api| {
      #[cfg(mobile)]
      let push = mobile::init(app, api)?;
      #[cfg(desktop)]
      let push = desktop::init(app, api)?;
      app.manage(push);
      Ok(())
    })
    .build()
}
