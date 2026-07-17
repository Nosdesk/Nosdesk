package com.nosdesk.plugin.push

import android.content.Intent

/** The context-only fields FCM delivers in a tapped notification's `data`. */
data class TappedNotification(
    val ndType: String?,
    val entityType: String?,
    val entityId: Int?,
    val ticketId: Int?,
)

/**
 * Buffers the last tapped notification for the JS layer to drain via
 * `get_pending_notification` — the single source of truth for deep-linking,
 * mirroring the iOS side. Populated from the launch intent (cold-start tap) and
 * `onNewIntent` (warm tap); FCM puts the message's `data` payload into the
 * intent extras as strings when the user taps a notification.
 */
object PendingNotification {
    @Volatile
    private var pending: TappedNotification? = null

    fun capture(intent: Intent?) {
        val extras = intent?.extras ?: return
        val ndType = extras.getString("nd_type")
        val entityType = extras.getString("entity_type")
        val ticketId = extras.getString("ticket_id")
        // Only our FCM notifications carry these keys; ignore other intents.
        if (ndType == null && entityType == null && ticketId == null) return
        pending = TappedNotification(
            ndType = ndType,
            entityType = entityType,
            entityId = extras.getString("entity_id")?.toIntOrNull(),
            ticketId = ticketId?.toIntOrNull(),
        )
    }

    /** Read-and-clear: whichever trigger drains first wins, the rest are no-ops. */
    fun take(): TappedNotification? {
        val current = pending
        pending = null
        return current
    }
}
