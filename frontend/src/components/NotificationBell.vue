<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue';
import { useRouter } from 'vue-router';
import { useSSE } from '@/services/sseService';
import { useAuthStore } from '@/stores/auth';
import {
  getNotifications,
  getUnreadCount,
  markNotificationsRead,
  markAllNotificationsRead,
  deleteNotifications,
  type Notification,
} from '@/services/notificationService';
import type { NotificationReceivedEventData } from '@/types/sse';
import { unwrapEventData } from '@/types/sse';
import UserAvatar from './UserAvatar.vue';

const router = useRouter();
const authStore = useAuthStore();
const { addEventListener, removeEventListener, connect, isConnected } = useSSE();

// State
const isOpen = ref(false);
const notifications = ref<Notification[]>([]);
const unreadCount = ref(0);
const isLoading = ref(false);
const dropdownRef = ref<HTMLElement | null>(null);
const buttonRef = ref<HTMLElement | null>(null);

// Format relative time
const formatRelativeTime = (dateString: string): string => {
  const date = new Date(dateString);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMs / 3600000);
  const diffDays = Math.floor(diffMs / 86400000);

  if (diffMins < 1) return 'Just now';
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays < 7) return `${diffDays}d ago`;
  return date.toLocaleDateString();
};

// Get icon for notification type
const getNotificationIcon = (type: string) => {
  switch (type) {
    case 'ticket_assigned':
      return 'user-plus';
    case 'ticket_status_changed':
      return 'refresh-cw';
    case 'comment_added':
      return 'message-circle';
    case 'mentioned':
      return 'at-sign';
    case 'ticket_created_requester':
      return 'plus-circle';
    default:
      return 'bell';
  }
};

// Fetch notifications
const fetchNotifications = async () => {
  try {
    isLoading.value = true;
    const [notifs, count] = await Promise.all([
      getNotifications({ limit: 10 }),
      getUnreadCount(),
    ]);
    notifications.value = notifs;
    unreadCount.value = count;
  } catch (error) {
    console.error('Failed to fetch notifications:', error);
  } finally {
    isLoading.value = false;
  }
};

// Handle new notification from SSE
const handleNewNotification = (rawData: unknown) => {
  try {
    const data = unwrapEventData(rawData as NotificationReceivedEventData);

    // Only handle notifications for current user
    if (!authStore.user?.uuid || authStore.user.uuid !== data.recipient_uuid) {
      return;
    }

    // Increment unread count
    unreadCount.value++;

    // If dropdown is open, refresh the list
    if (isOpen.value) {
      fetchNotifications();
    }
  } catch (error) {
    console.error('Error handling notification event:', error);
  }
};

// Toggle dropdown
const toggleDropdown = () => {
  isOpen.value = !isOpen.value;
  if (isOpen.value) {
    fetchNotifications();
  }
};

// Close dropdown
const closeDropdown = () => {
  isOpen.value = false;
};

// Handle click outside
const handleClickOutside = (event: MouseEvent) => {
  if (
    dropdownRef.value &&
    buttonRef.value &&
    !dropdownRef.value.contains(event.target as Node) &&
    !buttonRef.value.contains(event.target as Node)
  ) {
    closeDropdown();
  }
};

// Navigate to notification entity
const navigateToNotification = async (notification: Notification) => {
  // Mark as read
  if (!notification.is_read) {
    try {
      await markNotificationsRead([notification.id]);
      notification.is_read = true;
      unreadCount.value = Math.max(0, unreadCount.value - 1);
    } catch (error) {
      console.error('Failed to mark notification as read:', error);
    }
  }

  closeDropdown();

  // Navigate to the entity - use ticket_id from metadata if available
  if (notification.entity_type === 'ticket' || notification.entity_type === 'comment') {
    const ticketId = notification.metadata?.ticket_id ?? notification.entity_id;
    router.push(`/tickets/${ticketId}`);
  }
};

// Mark all as read
const handleMarkAllRead = async () => {
  try {
    await markAllNotificationsRead();
    notifications.value.forEach(n => n.is_read = true);
    unreadCount.value = 0;
  } catch (error) {
    console.error('Failed to mark all as read:', error);
  }
};

// Clear/delete a notification
const handleClearNotification = async (event: Event, notification: Notification) => {
  event.stopPropagation();
  try {
    await deleteNotifications([notification.id]);
    // Remove from list
    notifications.value = notifications.value.filter(n => n.id !== notification.id);
    // Decrement unread count if it was unread
    if (!notification.is_read) {
      unreadCount.value = Math.max(0, unreadCount.value - 1);
    }
  } catch (error) {
    console.error('Failed to clear notification:', error);
  }
};

// Lifecycle
onMounted(() => {
  fetchNotifications();
  // Ensure SSE connection is established for real-time notifications
  if (!isConnected.value) {
    connect();
  }
  addEventListener('notification-received', handleNewNotification);
  document.addEventListener('click', handleClickOutside);
});

onUnmounted(() => {
  removeEventListener('notification-received', handleNewNotification);
  document.removeEventListener('click', handleClickOutside);
});

// Computed
const hasUnread = computed(() => unreadCount.value > 0);
const displayCount = computed(() => unreadCount.value > 99 ? '99+' : String(unreadCount.value));
</script>

<template>
  <div class="relative">
    <!-- Bell Button -->
    <button
      ref="buttonRef"
      @click="toggleDropdown"
      class="relative p-2 text-secondary hover:text-primary hover:bg-surface-alt rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-accent"
      aria-label="Notifications"
      :aria-expanded="isOpen"
    >
      <!-- Bell Icon -->
      <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9" />
      </svg>

      <!-- Unread Badge -->
      <span
        v-if="hasUnread"
        class="absolute -top-0.5 -right-0.5 flex items-center justify-center min-w-[18px] h-[18px] px-1 text-xs font-bold text-white bg-status-error rounded-full"
      >
        {{ displayCount }}
      </span>
    </button>

    <!-- Dropdown Panel -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition ease-out duration-100"
        enter-from-class="transform opacity-0 scale-95"
        enter-to-class="transform opacity-100 scale-100"
        leave-active-class="transition ease-in duration-75"
        leave-from-class="transform opacity-100 scale-100"
        leave-to-class="transform opacity-0 scale-95"
      >
        <div
          v-if="isOpen"
          ref="dropdownRef"
          class="fixed z-[9999] w-80 sm:w-96 bg-surface border border-default rounded-xl shadow-xl overflow-hidden"
          :style="{
            top: buttonRef ? `${buttonRef.getBoundingClientRect().bottom + 8}px` : '60px',
            right: '16px',
          }"
        >
          <!-- Header -->
          <div class="flex items-center justify-between px-4 py-3 border-b border-default bg-surface-alt">
            <h3 class="font-semibold text-primary">Notifications</h3>
            <button
              v-if="hasUnread"
              @click="handleMarkAllRead"
              class="text-xs text-accent hover:text-accent-hover font-medium"
            >
              Mark all as read
            </button>
          </div>

          <!-- Notification List -->
          <div class="max-h-[400px] overflow-y-auto">
            <!-- Loading State -->
            <div v-if="isLoading" class="flex items-center justify-center py-8">
              <div class="animate-spin rounded-full h-6 w-6 border-2 border-accent border-t-transparent"></div>
            </div>

            <!-- Empty State -->
            <div v-else-if="notifications.length === 0" class="flex flex-col items-center justify-center py-8 text-tertiary">
              <svg class="w-12 h-12 mb-2 opacity-50" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9" />
              </svg>
              <p class="text-sm">No notifications yet</p>
            </div>

            <!-- Notification Items -->
            <div v-else>
              <button
                v-for="notification in notifications"
                :key="notification.id"
                @click="navigateToNotification(notification)"
                class="group w-full px-4 py-3 flex items-start gap-3 hover:bg-surface-alt transition-colors text-left border-b border-default last:border-b-0"
                :class="{ 'bg-accent/5': !notification.is_read }"
              >
                <!-- Icon -->
                <div
                  class="flex-shrink-0 w-8 h-8 rounded-full flex items-center justify-center"
                  :class="notification.is_read ? 'bg-surface-alt text-tertiary' : 'bg-accent/10 text-accent'"
                >
                  <!-- Ticket Assigned -->
                  <svg v-if="notification.notification_type === 'ticket_assigned'" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M18 9v3m0 0v3m0-3h3m-3 0h-3m-2-5a4 4 0 11-8 0 4 4 0 018 0zM3 20a6 6 0 0112 0v1H3v-1z" />
                  </svg>
                  <!-- Status Changed -->
                  <svg v-else-if="notification.notification_type === 'ticket_status_changed'" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                  </svg>
                  <!-- Comment Added -->
                  <svg v-else-if="notification.notification_type === 'comment_added'" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
                  </svg>
                  <!-- Mentioned -->
                  <svg v-else-if="notification.notification_type === 'mentioned'" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M16 12a4 4 0 10-8 0 4 4 0 008 0zm0 0v1.5a2.5 2.5 0 005 0V12a9 9 0 10-9 9m4.5-1.206a8.959 8.959 0 01-4.5 1.207" />
                  </svg>
                  <!-- Default Bell -->
                  <svg v-else class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9" />
                  </svg>
                </div>

                <!-- Content -->
                <div class="flex-1 min-w-0">
                  <p class="text-sm font-medium text-primary truncate">
                    {{ notification.title }}
                  </p>
                  <p v-if="notification.body" class="text-xs text-secondary line-clamp-2 mt-0.5">
                    {{ notification.body }}
                  </p>
                  <p class="text-xs text-tertiary mt-1">
                    {{ formatRelativeTime(notification.created_at) }}
                  </p>
                </div>

                <!-- Clear button and unread indicator -->
                <div class="flex-shrink-0 flex items-center gap-1">
                  <button
                    @click="handleClearNotification($event, notification)"
                    class="p-1 text-tertiary hover:text-primary hover:bg-surface-alt rounded transition-colors opacity-0 group-hover:opacity-100"
                    title="Clear notification"
                  >
                    <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                  <div
                    v-if="!notification.is_read"
                    class="w-2 h-2 rounded-full bg-accent"
                  ></div>
                </div>
              </button>
            </div>
          </div>

          <!-- Footer -->
          <div class="px-4 py-2 border-t border-default bg-surface-alt">
            <router-link
              to="/profile/settings/notifications"
              @click="closeDropdown"
              class="text-xs text-accent hover:text-accent-hover font-medium"
            >
              Notification settings
            </router-link>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.line-clamp-2 {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
</style>
