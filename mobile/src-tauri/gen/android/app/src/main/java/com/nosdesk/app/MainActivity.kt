package com.nosdesk.app

import android.os.Bundle
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import com.nosdesk.plugin.devicetext.DeviceTextPlugin

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  // Every WebView, including the new one a recreated activity builds (Tauri
  // only loads plugins against the first), gets the device text settings.
  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    DeviceTextPlugin.configure(webView)
  }
}
