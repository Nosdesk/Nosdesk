import { onMounted, onUnmounted, type Ref } from 'vue'
import { useSSE, type SSEEventType } from '@/services/sseService'
import { unwrapEventData } from '@/types/sse'

/**
 * Live-refresh hook for the ticket Activity Log.
 *
 * Follows the secondary-subscriber pattern (see `useLowStockSSE`): it
 * only registers/removes listeners and never touches the connection.
 * `useTicketSSE` owns the per-ticket `EventSource` at the `TicketView`
 * level, so the connection (and its `ticket-<id>` topic) already
 * exists by the time the activity feed mounts inside it.
 *
 * On any ticket-scoped event that corresponds to a new `sync_actions`
 * row (filtered to `ticketId`), it debounces and calls `onActivity`.
 * The component implements that as a fetch of events newer than its
 * current head — we refetch rather than reconstruct entries
 * client-side so the server stays authoritative for phrasing,
 * ordering, and `correlation_id` (which drives grouping). A single
 * user action can emit several events (a multi-field save, or a
 * comment that auto-adds a watcher); the debounce collapses the burst
 * into one refetch.
 *
 * The SSE layer suppresses echoes of the current client's own writes
 * (`source_client_id`), so this fires for *other* users' changes; the
 * local UI already reflects the current user's own edits.
 */

// Events that imply a new activity row on a ticket's timeline.
// Over-inclusion is harmless — the newest-slice refetch returns
// nothing when there's actually no new row.
const ACTIVITY_EVENTS: SSEEventType[] = [
  'ticket-updated',
  'comment-added',
  'comment-deleted',
  'ticket-linked',
  'ticket-unlinked',
  'project-assigned',
  'project-unassigned',
]

export function useTicketActivitySSE(
  ticketId: Ref<number | undefined>,
  onActivity: () => void,
  debounceMs = 400,
) {
  const { addEventListener, removeEventListener } = useSSE()
  let timer: ReturnType<typeof setTimeout> | null = null

  function schedule() {
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => {
      timer = null
      onActivity()
    }, debounceMs)
  }

  function handle(raw: unknown) {
    const tid = ticketId.value
    if (tid == null) return
    // All listed events carry ticket_id; link events also carry
    // linked_ticket_id (they fire for both endpoints). Refresh when
    // either side is the open ticket.
    const data = unwrapEventData(raw as { ticket_id?: number; linked_ticket_id?: number })
    if (data?.ticket_id === tid || data?.linked_ticket_id === tid) {
      schedule()
    }
  }

  onMounted(() => {
    ACTIVITY_EVENTS.forEach((e) => addEventListener(e, handle))
  })

  onUnmounted(() => {
    ACTIVITY_EVENTS.forEach((e) => removeEventListener(e, handle))
    if (timer) clearTimeout(timer)
  })
}
