mod asset_proxy;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    // Native HTTP client for the REST API (scoped by capabilities/default.json).
    .plugin(tauri_plugin_http::init())
    // System-browser OAuth (ASWebAuthenticationSession) for native OIDC login.
    .plugin(tauri_plugin_web_auth::init())
    // iOS Universal Links / Android App Links for ticket deep links; the JS
    // side listens via onOpenUrl (see mobile/src). Config in tauri.conf.json.
    .plugin(tauri_plugin_deep_link::init())
    // System-browser opener for the cross-tenant deep-link fallback.
    .plugin(tauri_plugin_opener::init())
    // Keystore/Keychain-backed storage for the auth refresh token.
    .plugin(tauri_plugin_secure_store::init())
    // APNs/FCM push device-token registration (iOS live, Android stubbed).
    .plugin(tauri_plugin_push::init())
    // Haptic feedback for the pull-to-refresh arm tick. iOS/Android only;
    // on desktop the commands error and the JS facade swallows it.
    .plugin(tauri_plugin_haptics::init())
    // Platform + OS version, used to name this device in the user's session
    // list (see mobile/src/deviceName.ts).
    .plugin(tauri_plugin_os::init())
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
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use tauri::Manager;
        if let Some(webview) = app.get_webview_window("main") {
          let _ = webview.with_webview(|pw| unsafe {
            let wk = pw.inner() as *mut AnyObject;
            let _: () = msg_send![wk, setAllowsBackForwardNavigationGestures: true];

            // Paint every layer (webview, scroll view, superview, window) with
            // the appearance-aware `LaunchBackground` colour (light / dark) so
            // nothing white shows before the content paints or in a region it
            // doesn't cover (launch flash, safe-area sliver). Keep the webview
            // opaque so its back/forward snapshots carry a background colour.
            let name: *mut AnyObject = msg_send![
              class!(NSString),
              stringWithUTF8String: b"LaunchBackground\0".as_ptr() as *const std::os::raw::c_char
            ];
            let color: *mut AnyObject = msg_send![class!(UIColor), colorNamed: name];
            if !color.is_null() {
              let _: () = msg_send![wk, setOpaque: true];
              let _: () = msg_send![wk, setBackgroundColor: color];
              // `underPageBackgroundColor` (iOS 15+) is what the interactive
              // swipe-back paints when the previous view's snapshot surface has
              // been reclaimed (common on a heavy page). Without it that
              // fallback is hard-coded white; with it, an evicted preview
              // degrades to the theme colour. Guarded for the iOS 14 floor.
              let responds: bool = msg_send![
                wk,
                respondsToSelector: objc2::sel!(setUnderPageBackgroundColor:)
              ];
              if responds {
                let _: () = msg_send![wk, setUnderPageBackgroundColor: color];
              }
              let scroll: *mut AnyObject = msg_send![wk, scrollView];
              if !scroll.is_null() {
                let _: () = msg_send![scroll, setBackgroundColor: color];
                // `.never` (raw 2): full-bleed content so CSS owns the
                // safe-area inset; `.automatic` leaves an uncovered bottom
                // strip that shows through as a white sliver.
                let _: () = msg_send![scroll, setContentInsetAdjustmentBehavior: 2_i64];
              }
              // Base colour behind the webview, for rotation / first layout.
              let superview: *mut AnyObject = msg_send![wk, superview];
              if !superview.is_null() {
                let _: () = msg_send![superview, setBackgroundColor: color];
              }
              let window: *mut AnyObject = msg_send![wk, window];
              if !window.is_null() {
                let _: () = msg_send![window, setBackgroundColor: color];
              }
            }
          });
        }
      }
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
