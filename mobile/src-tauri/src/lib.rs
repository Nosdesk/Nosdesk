mod keychain;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    // Native HTTP client for the REST API (scoped by capabilities/default.json).
    .plugin(tauri_plugin_http::init())
    // System-browser OAuth (ASWebAuthenticationSession) for native OIDC login.
    .plugin(tauri_plugin_web_auth::init())
    // OS-keychain storage for the auth refresh token.
    .invoke_handler(tauri::generate_handler![
      keychain::secure_store_save,
      keychain::secure_store_load,
      keychain::secure_store_clear,
    ])
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
