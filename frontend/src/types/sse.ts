/**
 * SSE Event Data Types
 * Type definitions for Server-Sent Events in ticket context
 */

import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions'
import type { UserInfo } from './user'
import type { Attachment } from './comment'

/**
 * Base wrapper for SSE events that may have nested data
 */
interface SSEEventWrapper<T> {
  data?: T
}

/**
 * Ticket field update values - discriminated by field type
 */
export type TicketFieldValue =
  | string
  | number
  | TicketStatus
  | TicketPriority
  | { uuid: string; user_info?: UserInfo }

/**
 * ticket-updated event data
 */
export interface TicketUpdatedEventData {
  ticket_id: number
  field:
    | 'title'
    | 'status'
    | 'workflow_state_id'
    | 'priority'
    | 'modified'
    | 'requester'
    | 'assignee'
  value: TicketFieldValue
  updated_by?: string
}

/**
 * Comment data from SSE events
 */
export interface SSECommentData {
  id: number
  content: string
  user_uuid?: string
  user_id?: string
  createdAt?: string
  created_at?: string
  ticket_id: number
  attachments?: Attachment[]
  user?: UserInfo
}

/**
 * comment-added event data
 */
export interface CommentAddedEventData {
  ticket_id: number
  comment: SSECommentData
}

/**
 * comment-deleted event data
 */
export interface CommentDeletedEventData {
  ticket_id: number
  comment_id: number
}

/**
 * device-linked / device-unlinked event data
 */
export interface DeviceLinkEventData {
  ticket_id: number
  device_id: number
}

/**
 * device-updated event data
 */
export interface DeviceUpdatedEventData {
  device_id: number
  field: string
  value: unknown
}

/**
 * device-created event data
 */
export interface DeviceCreatedEventData {
  device_id: number
  device: Record<string, unknown>
}

/**
 * device-deleted event data
 */
export interface DeviceDeletedEventData {
  device_id: number
}

/**
 * ticket-linked / ticket-unlinked event data
 */
export interface TicketLinkEventData {
  ticket_id: number
  linked_ticket_id: number
}

/**
 * ticket-deleted event data
 */
export interface TicketDeletedEventData {
  ticket_id: number
}

/**
 * project-assigned / project-unassigned event data
 */
export interface ProjectEventData {
  ticket_id: number
  project_id: number
}

/**
 * One viewer in a ticket's active-presence set. Keyed by user
 * uuid, not session, so multi-tab from the same user collapses to
 * a single entry. `last_active_at` is server-stamped UTC ISO 8601;
 * the avatar stack uses it to order most-recent first.
 */
export interface ViewerInfo {
  user_uuid: string
  last_active_at: string
}

/**
 * viewers-changed event data. Carries the full viewer set rather
 * than a delta so a fresh subscriber gets correct state on the
 * first event without needing a snapshot endpoint.
 */
export interface ViewersChangedEventData {
  ticket_id: number
  viewers: ViewerInfo[]
}

/**
 * Actor who triggered a notification
 */
export interface NotificationActor {
  uuid: string
  name: string
  avatar_thumb?: string
}

/**
 * notification-received event data
 */
export interface NotificationReceivedEventData {
  recipient_uuid: string
  notification: {
    id: string
    notification_type: string
    title: string
    body?: string
    entity_type: string
    entity_id: number
    ticket_id: number
    actor: NotificationActor
    metadata?: Record<string, unknown>
    timestamp: string
  }
}

/**
 * Union type of all SSE event data types (for generic handling)
 */
export type SSEEventData =
  | TicketUpdatedEventData
  | TicketDeletedEventData
  | CommentAddedEventData
  | CommentDeletedEventData
  | DeviceLinkEventData
  | DeviceUpdatedEventData
  | DeviceCreatedEventData
  | DeviceDeletedEventData
  | TicketLinkEventData
  | ProjectEventData
  | ViewersChangedEventData
  | NotificationReceivedEventData

/**
 * SSE event handler function type
 */
export type SSEEventHandler<T> = (data: T | SSEEventWrapper<T>) => void | Promise<void>

/**
 * Helper to unwrap SSE event data (handles both wrapped and direct formats)
 */
export function unwrapEventData<T>(data: T | SSEEventWrapper<T>): T {
  if (data && typeof data === 'object' && 'data' in data && data.data !== undefined) {
    return data.data as T
  }
  return data as T
}
