package com.nosdesk.plugin.push

import android.Manifest
import android.app.Activity
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.app.NotificationManagerCompat
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.firebase.messaging.FirebaseMessaging

const val NOTIFICATION_CHANNEL_ID = "nosdesk_notifications"
private const val ALIAS_NOTIFICATIONS = "postNotification"

/**
 * Android push plugin (FCM). Mirrors the iOS plugin's contract:
 * - `request_permission` — prompt for POST_NOTIFICATIONS (Android 13+; a no-op
 *   grant below that), then report the result.
 * - `get_token` — the FCM registration token.
 * - `get_pending_notification` — drain the buffered tap for deep-linking.
 *
 * Notification display is FCM's (the backend sends a `notification` block, so a
 * backgrounded message auto-displays and the tap opens the app with the `data`
 * as intent extras); this plugin captures that intent (cold start in `load`,
 * warm tap in `onNewIntent`) into [PendingNotification]. Foreground display +
 * token refresh live in [PushMessagingService].
 */
@TauriPlugin(
    permissions = [
        Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = ALIAS_NOTIFICATIONS),
    ]
)
class PushPlugin(private val activity: Activity) : Plugin(activity) {

    override fun load(webView: android.webkit.WebView) {
        super.load(webView)
        createDefaultChannel(activity)
        // Cold-start tap: the launcher intent carries the FCM `data` extras.
        PendingNotification.capture(activity.intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // Warm tap (singleTask activity re-used): capture the new intent.
        PendingNotification.capture(intent)
    }

    @Command
    fun requestPermission(invoke: Invoke) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            getPermissionState(ALIAS_NOTIFICATIONS) == PermissionState.GRANTED
        ) {
            resolveGranted(invoke, true)
            return
        }
        requestPermissionForAlias(ALIAS_NOTIFICATIONS, invoke, "permissionCallback")
    }

    @PermissionCallback
    fun permissionCallback(invoke: Invoke) {
        resolveGranted(invoke, getPermissionState(ALIAS_NOTIFICATIONS) == PermissionState.GRANTED)
    }

    /**
     * Report permission WITHOUT prompting, so opening notification settings
     * never triggers the runtime dialog as a side effect.
     *
     * `areNotificationsEnabled` is the authority on whether anything will
     * actually be shown: it also covers a user switching notifications off in
     * system settings, and channel-level blocks, which the POST_NOTIFICATIONS
     * grant alone does not. The permission state only distinguishes "never
     * asked" from "denied", and only on 13+.
     */
    @Command
    fun checkPermission(invoke: Invoke) {
        val enabled = NotificationManagerCompat.from(activity).areNotificationsEnabled()
        // PROMPT_WITH_RATIONALE counts as promptable: Android will still show
        // the dialog in that state, so the in-app button works and there is no
        // need to send the user out to system settings.
        val promptable = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            getPermissionState(ALIAS_NOTIFICATIONS) in
            setOf(PermissionState.PROMPT, PermissionState.PROMPT_WITH_RATIONALE)
        val status = when {
            enabled -> "granted"
            promptable -> "prompt"
            else -> "denied"
        }
        val ret = JSObject()
        ret.put("status", status)
        invoke.resolve(ret)
    }

    /**
     * Open this app's notification settings. After a denial the runtime prompt
     * stops appearing, so this is the route back.
     */
    @Command
    fun openSettings(invoke: Invoke) {
        val intent = Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, activity.packageName)
        try {
            activity.startActivity(intent)
        } catch (_: Exception) {
            // Not every OEM ships the per-app notification screen; the app
            // details page exists everywhere and gets them to the same place.
            activity.startActivity(
                Intent(
                    Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                    Uri.fromParts("package", activity.packageName, null),
                )
            )
        }
        invoke.resolve()
    }

    private fun resolveGranted(invoke: Invoke, granted: Boolean) {
        val ret = JSObject()
        ret.put("granted", granted)
        invoke.resolve(ret)
    }

    @Command
    fun getToken(invoke: Invoke) {
        FirebaseMessaging.getInstance().token.addOnCompleteListener { task ->
            val ret = JSObject()
            ret.put("token", if (task.isSuccessful) task.result else null)
            invoke.resolve(ret)
        }
    }

    @Command
    fun getPendingNotification(invoke: Invoke) {
        val pending = PendingNotification.take()
        val ret = JSObject()
        ret.put("ndType", pending?.ndType)
        ret.put("entityType", pending?.entityType)
        ret.put("entityId", pending?.entityId)
        ret.put("ticketId", pending?.ticketId)
        ret.put("ticketNumber", pending?.ticketNumber)
        invoke.resolve(ret)
    }

    companion object {
        /** The default channel FCM notifications post to (Android 8+ requires one). */
        fun createDefaultChannel(context: Context) {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
            val manager =
                context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            if (manager.getNotificationChannel(NOTIFICATION_CHANNEL_ID) != null) return
            val channel = NotificationChannel(
                NOTIFICATION_CHANNEL_ID,
                "Notifications",
                NotificationManager.IMPORTANCE_HIGH,
            )
            manager.createNotificationChannel(channel)
        }
    }
}
