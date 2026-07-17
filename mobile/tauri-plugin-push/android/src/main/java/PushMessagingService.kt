package com.nosdesk.plugin.push

import android.app.PendingIntent
import android.content.Intent
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

/**
 * FCM service. A BACKGROUNDED message auto-displays (the backend sends a
 * `notification` block) and Firebase handles it; this service only covers the
 * FOREGROUND case (Firebase doesn't auto-display then) by building the same
 * notification, wiring its tap to re-open the app with the `data` extras so
 * deep-linking still works. Token refresh is a no-op — the app re-reads the
 * token via `get_token` on each login/launch.
 */
class PushMessagingService : FirebaseMessagingService() {

    override fun onMessageReceived(message: RemoteMessage) {
        PushPlugin.createDefaultChannel(this)
        val title = message.notification?.title ?: return
        val body = message.notification?.body

        val launch = packageManager.getLaunchIntentForPackage(packageName)?.apply {
            addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            // Carry the FCM `data` so a foreground-tap deep-links like a background one.
            message.data.forEach { (key, value) -> putExtra(key, value) }
        }
        val contentIntent = PendingIntent.getActivity(
            this,
            0,
            launch,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        val builder = NotificationCompat.Builder(this, NOTIFICATION_CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle(title)
            .setAutoCancel(true)
            .setContentIntent(contentIntent)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
        if (body != null) builder.setContentText(body)

        NotificationManagerCompat.from(this)
            .notify(message.messageId?.hashCode() ?: 0, builder.build())
    }
}
