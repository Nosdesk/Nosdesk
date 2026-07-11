/**
 * Notification Service
 *
 * API client for notification preferences and history.
 */

import apiClient from '../apiClient';

export interface NotificationPreference {
  notification_type: string;
  channel: string;
  enabled: boolean;
}

export interface NotificationPreferencesResponse {
  preferences: NotificationPreference[];
  notification_types: NotificationType[];
}

export interface NotificationType {
  code: string;
  name: string;
  description: string | null;
  category: string;
}

export interface Notification {
  id: number;
  uuid: string;
  notification_type: string;
  title: string;
  body: string | null;
  entity_type: string;
  entity_id: number;
  is_read: boolean;
  /** Engagement state (phase 1 state model). `seen_at` drives the badge
   *  (unseen count), `archived_at` hides from the active inbox without
   *  deleting, `snoozed_until` defers re-surfacing. */
  seen_at?: string | null;
  archived_at?: string | null;
  snoozed_until?: string | null;
  created_at: string;
  metadata?: {
    ticket_id?: number;
    preview?: string;
    rule_name?: string;
    [key: string]: unknown;
  } | null;
}

/**
 * Notification type definitions for the UI
 */
export const NOTIFICATION_TYPES = [
  {
    code: 'ticket_assigned',
    name: 'Ticket Assigned',
    description: 'When you are assigned to a ticket',
    category: 'ticket',
    icon: 'user-plus',
  },
  {
    code: 'ticket_status_changed',
    name: 'Status Changed',
    description: 'When a ticket you\'re involved with changes status',
    category: 'ticket',
    icon: 'refresh-cw',
  },
  {
    code: 'comment_added',
    name: 'New Comment',
    description: 'When someone comments on your ticket',
    category: 'comment',
    icon: 'message-circle',
  },
  {
    code: 'mentioned',
    name: 'Mentioned',
    description: 'When someone mentions you in a comment',
    category: 'mention',
    icon: 'at-sign',
  },
  {
    code: 'ticket_created_requester',
    name: 'Ticket Created',
    description: 'When a ticket is created on your behalf',
    category: 'ticket',
    icon: 'plus-circle',
  },
  {
    code: 'doc_page_updated',
    name: 'Page Updated',
    description: 'When a documentation page you subscribe to is modified',
    category: 'documentation',
    icon: 'file-text',
  },
] as const;

/**
 * Channel definitions for the UI
 */
export const NOTIFICATION_CHANNELS = [
  {
    code: 'in_app',
    name: 'In-App',
    description: 'Toast notifications while using the app',
    icon: 'bell',
  },
  {
    code: 'email',
    name: 'Email',
    description: 'Email notifications (rate limited)',
    icon: 'mail',
  },
] as const;

/**
 * Get user's notification preferences
 */
export async function getNotificationPreferences(): Promise<NotificationPreference[]> {
  const response = await apiClient.get<NotificationPreference[]>('/notifications/preferences');
  return response.data;
}

/**
 * Update a notification preference
 */
export async function updateNotificationPreference(
  notificationType: string,
  channel: string,
  enabled: boolean
): Promise<void> {
  await apiClient.put('/notifications/preferences', {
    notification_type: notificationType,
    channel,
    enabled,
  });
}

/**
 * Get user's notifications
 */
export async function getNotifications(params?: {
  limit?: number;
  offset?: number;
  unread_only?: boolean;
}): Promise<Notification[]> {
  const response = await apiClient.get<Notification[]>('/notifications', { params });
  return response.data;
}

/**
 * Get unread notification count
 */
export async function getUnreadCount(): Promise<number> {
  const response = await apiClient.get<{ count: number }>('/notifications/count');
  return response.data.count;
}

/**
 * Get unseen notification count (badge source; cleared on panel open,
 * distinct from unread).
 */
export async function getUnseenCount(): Promise<number> {
  const response = await apiClient.get<{ count: number }>('/notifications/unseen-count');
  return response.data.count;
}

/**
 * Mark all notifications as seen (badge clear on panel/inbox open).
 */
export async function markAllSeen(): Promise<void> {
  await apiClient.post('/notifications/seen');
}

/**
 * Mark notifications as read
 */
export async function markNotificationsRead(notificationIds: number[]): Promise<void> {
  await apiClient.post('/notifications/read', { notification_ids: notificationIds });
}

/**
 * Mark notifications unread (inverse of read).
 */
export async function markNotificationsUnread(notificationIds: number[]): Promise<void> {
  await apiClient.post('/notifications/unread', { notification_ids: notificationIds });
}

/**
 * Archive notifications (reversible; hides from the active inbox).
 */
export async function archiveNotifications(notificationIds: number[]): Promise<void> {
  await apiClient.post('/notifications/archive', { notification_ids: notificationIds });
}

/**
 * Unarchive notifications (restore to the active inbox).
 */
export async function unarchiveNotifications(notificationIds: number[]): Promise<void> {
  await apiClient.post('/notifications/unarchive', { notification_ids: notificationIds });
}

/**
 * Snooze notifications until `until` (an ISO-8601 instant): they hide
 * from the active inbox until then, then auto-unsnooze server-side.
 */
export async function snoozeNotifications(notificationIds: number[], until: string): Promise<void> {
  await apiClient.post('/notifications/snooze', { notification_ids: notificationIds, until });
}

/**
 * Mark all notifications as read
 */
export async function markAllNotificationsRead(): Promise<void> {
  await apiClient.post('/notifications/read-all');
}

/**
 * Delete notifications
 */
export async function deleteNotifications(notificationIds: number[]): Promise<void> {
  await apiClient.post('/notifications/delete', { notification_ids: notificationIds });
}

export default {
  getNotificationPreferences,
  updateNotificationPreference,
  deleteNotifications,
  getNotifications,
  getUnreadCount,
  getUnseenCount,
  markAllSeen,
  markNotificationsRead,
  markNotificationsUnread,
  markAllNotificationsRead,
  archiveNotifications,
  unarchiveNotifications,
  snoozeNotifications,
  NOTIFICATION_TYPES,
  NOTIFICATION_CHANNELS,
};
