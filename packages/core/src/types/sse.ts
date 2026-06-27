/**
 * SSE Event Data Types
 * Type definitions for Server-Sent Events in ticket context
 */

/**
 * Base wrapper for SSE events that may have nested data
 */
interface SSEEventWrapper<T> {
  data?: T
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
 * Fields the backend allows on the `/field-preview` endpoint.
 * Mirrors `PreviewableField` in `backend/src/handlers/tickets.rs`.
 * Kept as a literal union so call sites can't typo into a field
 * the backend will reject.
 */
export type PreviewableField = 'title' | 'resolution_notes'

/**
 * ticket-field-previewed event data. Transient in-flight value
 * broadcast during typing. Not persistent: the next `ticket-updated`
 * with the committed value is what consumers should rely on for
 * durable state.
 */
export interface TicketFieldPreviewedEventData {
  ticket_id: number
  field: PreviewableField
  value: string
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
 * Helper to unwrap SSE event data (handles both wrapped and direct formats)
 */
export function unwrapEventData<T>(data: T | SSEEventWrapper<T>): T {
  if (data && typeof data === 'object' && 'data' in data && data.data !== undefined) {
    return data.data as T
  }
  return data as T
}
