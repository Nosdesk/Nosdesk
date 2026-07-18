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
    // Keystore/Keychain-backed storage for the auth refresh token.
    .plugin(tauri_plugin_secure_store::init())
    // APNs/FCM push device-token registration (iOS live, Android stubbed).
    .plugin(tauri_plugin_push::init())
    // Haptic feedback for the pull-to-refresh arm tick. iOS/Android only;
    // on desktop the commands error and the JS facade swallows it.
    .plugin(tauri_plugin_haptics::init())
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

            // Paint the webview with the appearance-aware `LaunchBackground`
            // colour (light #f3f4f6 / dark #08090a, the same colorset the
            // launch storyboard uses) and make it non-opaque, so that
            // colour shows before the web content paints instead of the
            // default white backing. This is what removes the white launch
            // flash; the scroll view is painted too so overscroll doesn't
            // expose white. Colour resolved from the asset catalog by name.
            let name: *mut AnyObject = msg_send![
              class!(NSString),
              stringWithUTF8String: b"LaunchBackground\0".as_ptr() as *const std::os::raw::c_char
            ];
            let color: *mut AnyObject = msg_send![class!(UIColor), colorNamed: name];
            if !color.is_null() {
              let _: () = msg_send![wk, setOpaque: false];
              let _: () = msg_send![wk, setBackgroundColor: color];
              let scroll: *mut AnyObject = msg_send![wk, scrollView];
              if !scroll.is_null() {
                let _: () = msg_send![scroll, setBackgroundColor: color];
                // Stop iOS auto-adjusting the scroll view's content inset
                // to the safe area. With the default `.automatic`, the
                // native layer pads a strip at the bottom (home-indicator
                // area) that our full-bleed CSS (viewport-fit=cover +
                // env() padding) doesn't fill, so that strip shows the
                // window background as a white sliver. `.never` (raw value
                // 2) makes the web content full-bleed, so CSS owns all the
                // inset padding and there is no uncovered strip.
                let _: () = msg_send![scroll, setContentInsetAdjustmentBehavior: 2_i64];
              }
              // Paint the layers BEHIND the webview too: the host view
              // controller's view (webview superview) and the window.
              // A non-opaque webview reveals these in any region it
              // doesn't fully cover (the bottom safe-area / home-
              // indicator strip), which is where the white bottom
              // sliver came from.
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
