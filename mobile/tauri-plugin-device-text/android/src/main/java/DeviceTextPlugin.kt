package com.nosdesk.plugin.devicetext

import android.app.Activity
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

@InvokeArg
class EventHandlerArgs {
    lateinit var handler: Channel
}

/**
 * Reports the system font size and Bold Text to the webview.
 *
 * Android's WebView already multiplies every font size by the system font
 * scale through `textZoom`. That grows text inside boxes laid out for the
 * default size, and it would double any scale the app applies itself, so
 * `load` pins `textZoom` at 100 and the scale goes to the web layer instead.
 *
 * `scale` is `Configuration.fontScale`, capped at 2.0 (the 200% WCAG asks
 * for). `bold` is the Bold text setting, which Android exposes as a font weight
 * adjustment from API 31; older versions have no such setting.
 *
 * The manifest declares `fontScale|fontWeightAdjustment` in `configChanges`, so
 * a change arrives in `onConfigurationChanged` instead of recreating the
 * activity and reloading the webview.
 */
@TauriPlugin
class DeviceTextPlugin(private val activity: Activity) : Plugin(activity) {
    // Set from the command thread, read on the main thread.
    @Volatile private var handler: Channel? = null
    @Volatile private var lastSent: Pair<Double, Boolean>? = null

    override fun load(webView: WebView) {
        activity.runOnUiThread { webView.settings.textZoom = 100 }
    }

    /** Called once from Rust at startup with the channel changes go out on. */
    @Command
    fun setEventHandler(invoke: Invoke) {
        val args = invoke.parseArgs(EventHandlerArgs::class.java)
        handler = args.handler
        lastSent = read(activity.resources.configuration)
        invoke.resolve()
    }

    @Command
    fun get(invoke: Invoke) {
        invoke.resolve(toJs(read(activity.resources.configuration)))
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        sendIfChanged(read(newConfig))
    }

    override fun onResume(activity: AppCompatActivity) {
        sendIfChanged(read(activity.resources.configuration))
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

    private companion object {
        const val MAX_SCALE = 2.0
    }
}
