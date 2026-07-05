mod asset_proxy;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    // Native HTTP client for the REST API (scoped by capabilities/default.json).
    .plugin(tauri_plugin_http::init())
    // System-browser OAuth (ASWebAuthenticationSession) for native OIDC login.
    .plugin(tauri_plugin_web_auth::init())
    // Keystore/Keychain-backed storage for the auth refresh token.
    .plugin(tauri_plugin_secure_store::init())
    // Authenticated asset proxy: the webview loads workspace-scoped files via
    // the `nosdesk-asset` scheme; Rust forwards them to the API with the bearer
    // and Range header. See src/asset_proxy.rs.
    .manage(asset_proxy::AssetProxy::new())
    .register_asynchronous_uri_scheme_protocol(asset_proxy::SCHEME, asset_proxy::handle)
    .invoke_handler(tauri::generate_handler![asset_proxy::set_asset_proxy_session])
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      // Native iOS swipe-back: hand the left-edge back gesture to WebKit itself.
      // WKWebView drives its own interactive scrub (bitmap snapshot of the prior
      // page + its back/forward list) and fires `popstate`, which vue-router
      // handles. Requires the SPA to push real history entries. iOS-only, so the
      // desktop web build is untouched.
      #[cfg(target_os = "ios")]
      {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        use tauri::Manager;
        if let Some(webview) = app.get_webview_window("main") {
          let _ = webview.with_webview(|pw| unsafe {
            let wk = pw.inner() as *mut AnyObject;
            let _: () = msg_send![wk, setAllowsBackForwardNavigationGestures: true];
          });
        }
      }
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
