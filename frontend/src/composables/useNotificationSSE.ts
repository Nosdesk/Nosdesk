/**
 * useNotificationSSE - Composable for handling notification SSE events
 *
 * Listens for notification-received events and shows toast notifications.
 * Only shows notifications for the current user.
 */

import { onMounted, onUnmounted } from 'vue';
import { useSSE } from '@/services/sseService';
import { useAuthStore } from '@/stores/auth';
import { useToastStore } from '@/stores/toast';
import type { NotificationReceivedEventData } from '@/types/sse';
import { unwrapEventData } from '@/types/sse';

export function useNotificationSSE() {
  const { addEventListener, removeEventListener } = useSSE();
  const authStore = useAuthStore();
  const toastStore = useToastStore();

  const handleNotification = (rawData: unknown) => {
    try {
      const data = unwrapEventData(rawData as NotificationReceivedEventData);

      // Only show notifications for the current user
      if (!authStore.user?.uuid || authStore.user.uuid !== data.recipient_uuid) {
        return;
      }

      const { notification } = data;

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

  onMounted(() => {
    addEventListener('notification-received', handleNotification);
  });

  onUnmounted(() => {
    removeEventListener('notification-received', handleNotification);
  });

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
