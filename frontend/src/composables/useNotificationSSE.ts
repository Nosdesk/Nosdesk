/**
 * useNotificationSSE - toast + browser notification for the current user.
 *
 * Reacts to `notification` sync actions (the `sync_actions` change-stream)
 * rather than the legacy `notification-received` discrete event. The
 * stream is delivered cross-machine via Postgres NOTIFY, and the emit is
 * scoped to the recipient's private `user:<uuid>` group, so a client only
 * receives its own notifications regardless of which machine created them.
 * The action `data` is the full NotificationEvent (same shape the old
 * event carried), so the toast renders straight from it.
 */

import { useAuthStore } from '@/stores/auth';
import { useToastStore } from '@/stores/toast';
import { useSyncActions } from '@/composables/useSyncActions';
import type { NotificationReceivedEventData } from '@/types/sse';

type NotificationEventData = NotificationReceivedEventData['notification'];

export function useNotificationSSE() {
  const authStore = useAuthStore();
  const toastStore = useToastStore();

  const handleNotification = (rawData: unknown) => {
    try {
      // The sync action's data IS the NotificationEvent. Group scoping
      // already guarantees this client is the recipient; no client-side
      // recipient filter is needed.
      const notification = rawData as NotificationEventData;
      if (!authStore.user?.uuid || !notification) {
        return;
      }

      // Show toast notification
      toastStore.notification(
        notification.title,
        notification.body,
        notification.entity_type,
        notification.entity_id,
        notification.ticket_id,
        notification.actor.name,
        notification.actor.avatar_thumb
      );

      // Play notification sound if enabled (could be a user preference)
      playNotificationSound();

      // Request browser notification if permission granted
      showBrowserNotification(notification);
    } catch (error) {
      console.error('Error handling notification event:', error);
    }
  };

  const playNotificationSound = () => {
    // Optional: Play a subtle notification sound
    // This could be controlled by a user preference
    // For now, we'll skip this to keep it simple
  };

  const showBrowserNotification = (notification: NotificationReceivedEventData['notification']) => {
    // Only show browser notifications if permission is granted
    if ('Notification' in window && Notification.permission === 'granted') {
      try {
        new Notification(notification.title, {
          body: notification.body || undefined,
          icon: '/favicon.ico',
          tag: notification.id, // Prevents duplicate notifications
        });
      } catch (error) {
        // Browser notifications may fail in some contexts (e.g., incognito)
        console.debug('Browser notification failed:', error);
      }
    }
  };

  // One toast per notification action; no debounce (each is distinct).
  useSyncActions(
    (actions) => {
      for (const action of actions) {
        handleNotification(action.data);
      }
    },
    { aggregates: ['notification'] },
  );

  return {
    // Expose for testing
    handleNotification,
  };
}

/**
 * Request browser notification permission
 * Call this when the user explicitly enables notifications
 */
export async function requestNotificationPermission(): Promise<boolean> {
  if (!('Notification' in window)) {
    return false;
  }

  if (Notification.permission === 'granted') {
    return true;
  }

  if (Notification.permission === 'denied') {
    return false;
  }

  const permission = await Notification.requestPermission();
  return permission === 'granted';
}
