/**
 * Toast Store
 *
 * Manages toast notifications in the application.
 * Supports multiple types including notification toasts from the notification system.
 */

import { defineStore } from 'pinia';
import { ref, computed } from 'vue';

export type ToastType = 'info' | 'success' | 'warning' | 'error' | 'notification';

export interface Toast {
  id: string;
  type: ToastType;
  title: string;
  message?: string;
  duration: number;       // Auto-dismiss after ms (0 = persistent)
  dismissible: boolean;
  // For notification toasts
  notification?: {
    entityType: string;
    entityId: number;
    ticketId: number;
    actorName?: string;
    actorAvatar?: string;
  };
  createdAt: number;
}

const MAX_TOASTS = 5;
const DEFAULT_DURATION = 5000;
const NOTIFICATION_DURATION = 8000;

export const useToastStore = defineStore('toast', () => {
  const toasts = ref<Toast[]>([]);

  // Get visible toasts (most recent first, limited)
  const visibleToasts = computed(() =>
    toasts.value
      .slice()
      .sort((a, b) => b.createdAt - a.createdAt)
      .slice(0, MAX_TOASTS)
  );

  // Unread notification count (for badge)
  const notificationCount = computed(() =>
    toasts.value.filter((t) => t.type === 'notification').length
  );

  // Generate unique ID
  const generateId = (): string => {
    return `toast-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
  };

  // Add a toast
  function addToast(toast: Omit<Toast, 'id' | 'createdAt'>): string {
    const id = generateId();
    const newToast: Toast = {
      ...toast,
      id,
      createdAt: Date.now(),
    };

    toasts.value.push(newToast);

    // Auto-dismiss if duration > 0
    if (newToast.duration > 0) {
      setTimeout(() => {
        removeToast(id);
      }, newToast.duration);
    }

    return id;
  }

  // Remove a toast by ID
  function removeToast(id: string): void {
    const index = toasts.value.findIndex((t) => t.id === id);
    if (index !== -1) {
      toasts.value.splice(index, 1);
    }
  }

  // Clear all toasts
  function clearAll(): void {
    toasts.value = [];
  }

  // Convenience methods for different toast types

  function info(title: string, message?: string): string {
    return addToast({
      type: 'info',
      title,
      message,
      duration: DEFAULT_DURATION,
      dismissible: true,
    });
  }

  function success(title: string, message?: string): string {
    return addToast({
      type: 'success',
      title,
      message,
      duration: DEFAULT_DURATION,
      dismissible: true,
    });
  }

  function warning(title: string, message?: string): string {
    return addToast({
      type: 'warning',
      title,
      message,
      duration: DEFAULT_DURATION,
      dismissible: true,
    });
  }

  function error(title: string, message?: string): string {
    // Errors don't auto-dismiss
    return addToast({
      type: 'error',
      title,
      message,
      duration: 0,
      dismissible: true,
    });
  }

  /**
   * Show a notification toast from the notification system
   */
  function notification(
    title: string,
    message: string | undefined,
    entityType: string,
    entityId: number,
    ticketId: number,
    actorName?: string,
    actorAvatar?: string
  ): string {
    return addToast({
      type: 'notification',
      title,
      message,
      duration: NOTIFICATION_DURATION,
      dismissible: true,
      notification: {
        entityType,
        entityId,
        ticketId,
        actorName,
        actorAvatar,
      },
    });
  }

  return {
    // State
    toasts,
    visibleToasts,
    notificationCount,

    // Actions
    addToast,
    removeToast,
    clearAll,

    // Convenience methods
    info,
    success,
    warning,
    error,
    notification,
  };
});
