package com.nosdesk.plugin.devicetext

import android.app.Activity
import android.content.Context
import android.content.res.Configuration
import android.os.Build
import android.webkit.WebView
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.lang.ref.WeakReference

@InvokeArg
class EventHandlerArgs {
    lateinit var handler: Channel
}

/**
 * Reports the system font size and Bold Text to the webview.
 *
 * Android's WebView already multiplies every font size by the system font
 * scale through `textZoom`. That grows text inside boxes laid out for the
 * default size, and it would double any scale the app applies itself. Once the
 * web layer calls `follow` (it applies the scale itself from then on),
 * `textZoom` is pinned at 100 on this WebView and on every one created after
 * it. Until then the WebView keeps scaling text as it always has.
 *
 * `scale` is `Configuration.fontScale`, capped at 2.0 (the 200% WCAG asks
 * for). `bold` is the Bold text setting, which Android exposes as a font weight
 * adjustment from API 31; older versions have no such setting.
 *
 * The manifest declares `fontScale|fontWeightAdjustment` in `configChanges`, so
 * those changes arrive in `onConfigurationChanged` instead of recreating the
 * activity. Other changes (Display size) still recreate it, with a new
 * WebView that Tauri does not pass to `load`: `MainActivity.onWebViewCreate`
 * hands every one to [configure]. Settings are read from the application
 * context, never from an activity that may have been destroyed.
 */
@TauriPlugin
class DeviceTextPlugin(activity: Activity) : Plugin(activity) {
    private val appContext: Context = activity.applicationContext

    // Set from the command thread, read on the main thread.
    @Volatile private var handler: Channel? = null
    @Volatile private var lastSent: Pair<Double, Boolean>? = null

    override fun load(webView: WebView) {
        configure(webView)
    }

    /** Called once from Rust at startup with the channel changes go out on. */
    @Command
    fun setEventHandler(invoke: Invoke) {
        val args = invoke.parseArgs(EventHandlerArgs::class.java)
        handler = args.handler
        lastSent = read(appContext.resources.configuration)
        invoke.resolve()
    }

    @Command
    fun get(invoke: Invoke) {
        invoke.resolve(toJs(read(appContext.resources.configuration)))
    }

    /** The web layer now scales text itself: stop the WebView doing it too. */
    @Command
    fun follow(invoke: Invoke) {
        following = true
        currentWebView?.get()?.let { configure(it) }
        invoke.resolve()
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        sendIfChanged(read(newConfig))
    }

    override fun onResume(activity: AppCompatActivity) {
        sendIfChanged(read(appContext.resources.configuration))
    }

    private fun sendIfChanged(settings: Pair<Double, Boolean>) {
        val channel = handler ?: return
        if (settings == lastSent) return
        lastSent = settings
        channel.send(toJs(settings))
    }

    private fun read(config: Configuration): Pair<Double, Boolean> {
        val scale = minOf(config.fontScale.toDouble(), MAX_SCALE)
        val bold =
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
                config.fontWeightAdjustment != Configuration.FONT_WEIGHT_ADJUSTMENT_UNDEFINED &&
                config.fontWeightAdjustment > 0
        return Pair(scale, bold)
    }

    private fun toJs(settings: Pair<Double, Boolean>): JSObject {
        val result = JSObject()
        result.put("scale", settings.first)
        result.put("bold", settings.second)
        return result
    }

    companion object {
        private const val MAX_SCALE = 2.0

        // Process wide: the plugin outlives an activity, and so does following.
        @Volatile private var following = false
        @Volatile private var currentWebView: WeakReference<WebView>? = null

        /**
         * Sets up a WebView: pinch zoom without on-screen buttons and, once the
         * web layer follows the device text size, `textZoom` pinned at 100.
         * Call it for every WebView the activity creates.
         */
        @JvmStatic
        fun configure(webView: WebView) {
            currentWebView = WeakReference(webView)
            webView.post {
                val settings = webView.settings
                settings.setSupportZoom(true)
                settings.builtInZoomControls = true
                settings.displayZoomControls = false
                if (following) settings.textZoom = 100
            }
        }
    }
}
