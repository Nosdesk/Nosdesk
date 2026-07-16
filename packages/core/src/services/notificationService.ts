/**
 * Notification Service
 *
 * API client for notification preferences and history.
 */

import apiClient from '../apiClient';

/**
 * Per-cell delivery frequency (mirrors the backend `NotificationFrequency`):
 * - `instant` — deliver immediately on this channel
 * - `digest`  — batch into a periodic summary (email only; see `channelSupportsDigest`)
 * - `off`     — never deliver on this channel
 */
export type NotificationFrequency = 'instant' | 'digest' | 'off';

/** Whether `digest` is a meaningful frequency for a channel. Only the email
 *  channel batches into a summary; in-app is a live stream and push is
 *  instant-or-nothing, so the UI must not offer `digest` for those. Mirrors
 *  the backend `NotificationChannel::supports_digest`. */
export function channelSupportsDigest(channelCode: string): boolean {
  return channelCode === 'email';
}

export interface NotificationPreference {
  notification_type: string;
  notification_name: string;
  description: string | null;
  category: string;
  /**
   * Per-channel enabled state, keyed by channel code (`in_app`, `email`,
   * `push`). Kept for backward compatibility (`enabled = frequency != off`);
   * the frequency-aware UI reads `frequencies` instead. The API returns ONE
   * row per type with these maps, NOT one row per (type, channel) pair.
   */
  channels: Record<string, boolean>;
  /**
   * Per-channel effective frequency (`instant` | `digest` | `off`) after the
   * system → workspace → user inheritance resolution.
   */
  frequencies: Record<string, NotificationFrequency>;
  /**
   * Channels the workspace admin has locked for this type. A locked cell is
   * enforced by the workspace default and the user cannot override it — the
   * UI renders it read-only.
   */
  locked: Record<string, boolean>;
}

/**
 * A workspace admin's notification DEFAULT for one type (the middle layer of
 * the system → workspace → user inheritance). `frequencies` is the workspace
 * default per channel (falling back to the system default); `locked` marks
 * cells members cannot override. Mirrors `WorkspaceNotificationDefaultResponse`.
 */
export interface WorkspaceNotificationDefault {
  notification_type: string;
  notification_name: string;
  description: string | null;
  category: string;
  frequencies: Record<string, NotificationFrequency>;
  locked: Record<string, boolean>;
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
  {
    code: 'push',
    name: 'Push',
    description: 'Mobile push notifications',
    icon: 'bell',
  },
] as const;

/**
 * Get user's notification preferences (per-type rows with `frequencies` +
 * `locked` maps). The matrix UI renders directly from these rows.
 */
export async function getNotificationPreferences(): Promise<NotificationPreference[]> {
  const response = await apiClient.get<NotificationPreference[]>('/notifications/preferences');
  return response.data;
}

/**
 * Set the delivery frequency for one (type, channel) cell of the signed-in
 * user's preferences. `digest` coerces to `instant` on non-batching channels
 * server-side.
 */
export async function updateNotificationPreference(
  notificationType: string,
  channel: string,
  frequency: NotificationFrequency
): Promise<void> {
  await apiClient.put('/notifications/preferences', {
    notification_type: notificationType,
    channel,
    frequency,
  });
}

/**
 * Get the workspace's notification DEFAULTS matrix (admin-only). One row per
 * type with the effective default frequency + `locked` per channel.
 */
export async function getWorkspaceNotificationDefaults(): Promise<WorkspaceNotificationDefault[]> {
  const response = await apiClient.get<WorkspaceNotificationDefault[]>(
    '/admin/notification-defaults'
  );
  return response.data;
}

/**
 * Set one (type, channel) cell of the workspace default matrix (admin-only).
 * `locked` prevents members from overriding this cell.
 */
export async function updateWorkspaceNotificationDefault(
  notificationType: string,
  channel: string,
  frequency: NotificationFrequency,
  locked: boolean
): Promise<void> {
  await apiClient.put('/admin/notification-defaults', {
    notification_type: notificationType,
    channel,
    frequency,
    locked,
  });
}

/** Device platform for push registration — matches the backend's
 *  `user_push_devices.platform` CHECK (`ios` | `android` | `web`). */
export type PushPlatform = 'ios' | 'android' | 'web';

/**
 * Register (or refresh) the current device's push token so the backend can
 * deliver push notifications to it. Idempotent server-side (upsert on token).
 * Called from the mobile shell after the session is established.
 */
export async function registerPushDevice(
  platform: PushPlatform,
  token: string,
  appVersion?: string
): Promise<void> {
  await apiClient.post('/notifications/devices', {
    platform,
    token,
    app_version: appVersion,
  });
}

/**
 * Revoke this device's push token (on sign-out). Best-effort — a failure here
 * must not block sign-out; the backend also prunes tokens the provider rejects.
 */
export async function unregisterPushDevice(token: string): Promise<void> {
  await apiClient.delete(`/notifications/devices/${encodeURIComponent(token)}`);
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
  getWorkspaceNotificationDefaults,
  updateWorkspaceNotificationDefault,
  channelSupportsDigest,
  registerPushDevice,
  unregisterPushDevice,
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
