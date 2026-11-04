package ch.manaf.tauri_plugins.web_auth

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.webkit.WebView
import androidx.browser.customtabs.CustomTabsIntent
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import app.tauri.plugin.Invoke

@InvokeArg
class AuthenticateArgs {
    var url: String? = null
    var callbackScheme: String? = null
}

@TauriPlugin
class WebAuthPlugin(private val activity: Activity): Plugin(activity) {
    private var pendingInvoke: Invoke? = null
    private var callbackScheme: String? = null

    override fun load(webView: WebView) {
        val intent = activity.intent
        if (intent.action == Intent.ACTION_VIEW) {
            maybeHandleAuthCallback(intent)
        }

        super.load(webView)
    }

    override fun onNewIntent(intent: Intent) {
        if (intent.action == Intent.ACTION_VIEW) {
            maybeHandleAuthCallback(intent)
        }
    }

    @Command
    fun authenticate(invoke: Invoke) {
        val args = invoke.parseArgs(AuthenticateArgs::class.java)
        val url = args.url
        callbackScheme = args.callbackScheme

        if (url == null || callbackScheme == null) {
            invoke.reject("Invalid arguments")
            return
        }

        pendingInvoke = invoke

        val customTabsIntent = CustomTabsIntent.Builder().build()
        customTabsIntent.launchUrl(activity, Uri.parse(url))
    }

    // Internal: handle callback if intent matches the expected scheme and pendingInvoke is set
    private fun maybeHandleAuthCallback(intent: Intent?) {
        val data = intent?.data
        if (pendingInvoke != null && data != null && data.scheme == callbackScheme) {
            val ret = JSObject()
            ret.put("callbackUrl", data.toString())
            pendingInvoke?.resolve(ret)
            pendingInvoke = null
        }
    }
}
