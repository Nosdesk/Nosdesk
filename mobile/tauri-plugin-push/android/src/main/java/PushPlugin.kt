package com.nosdesk.plugin.push

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/**
 * Android push plugin — STUB until FCM is provisioned.
 *
 * `request_permission` reports whether `POST_NOTIFICATIONS` is granted (always
 * true below Android 13, which has no runtime notification permission).
 * `get_token` returns `null`: obtaining an FCM registration token needs the
 * Firebase SDK + `google-services.json`, which aren't set up yet. When they are,
 * add `firebase-messaging` to build.gradle.kts and return
 * `FirebaseMessaging.getInstance().token` here. The backend already routes
 * `android` device tokens to FCM (Step 4), so only this method changes.
 */
@TauriPlugin
class PushPlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun requestPermission(invoke: Invoke) {
        val granted = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            ContextCompat.checkSelfPermission(
                activity,
                Manifest.permission.POST_NOTIFICATIONS,
            ) == PackageManager.PERMISSION_GRANTED
        } else {
            true
        }
        val ret = JSObject()
        ret.put("granted", granted)
        invoke.resolve(ret)
    }

    @Command
    fun getToken(invoke: Invoke) {
        // No FCM yet — see class doc.
        val ret = JSObject()
        ret.put("token", null as String?)
        invoke.resolve(ret)
    }

    @Command
    fun getPendingNotification(invoke: Invoke) {
        // No FCM / tap handling yet — nothing pending.
        val ret = JSObject()
        ret.put("ndType", null as String?)
        ret.put("entityType", null as String?)
        ret.put("entityId", null as Int?)
        ret.put("ticketId", null as Int?)
        invoke.resolve(ret)
    }
}
