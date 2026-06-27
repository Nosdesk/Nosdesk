import { onUnmounted, type Ref } from 'vue'
import { useSyncActions } from '@/composables/useSyncActions'
import type { SyncAction } from '@nosdesk/core/sync/types'

/**
 * Live-refresh hook for the ticket Activity Log.
 *
 * The activity feed is itself a server-rendered view of `sync_actions`
 * for a ticket (`GET /api/tickets/:id/activity`), so it reacts to the
 * same change-stream via `useSyncActions` rather than the legacy
 * discrete SSE events. That stream is delivered to every backend
 * machine via Postgres NOTIFY, so the feed stays live across fly
 * machines, where the old discrete events were instance-local.
 *
 * On any sync action that belongs to this ticket's timeline it debounces
 * and calls `onActivity`. The component refetches the newest slice
 * rather than reconstructing entries client-side, so the server stays
 * authoritative for phrasing, ordering, and `correlation_id` grouping. A
 * single user action can emit several rows (a multi-field save, or a
 * comment that auto-adds a watcher); the debounce collapses the burst
 * into one refetch.
 *
 * Echoes of the current client's own writes are already reflected in the
 * local UI; the server-authoritative refetch reconciles either way.
 */

/**
 * Does this sync action belong to the given ticket's timeline? Ticket
 * aggregate rows carry the numeric id as `aggregate_id`; ticket-child
 * aggregates (comment, project_ticket, attachment, ...) carry it as
 * `data.ticket_id`, and link rows fire for both endpoints via
 * `data.linked_ticket_id`.
 */
function touchesTicket(action: SyncAction, ticketId: number): boolean {
  if (action.aggregate === 'ticket' && action.aggregate_id === String(ticketId)) {
    return true
  }
  const data = action.data as { ticket_id?: number; linked_ticket_id?: number }
  return data?.ticket_id === ticketId || data?.linked_ticket_id === ticketId
}

export function useTicketActivitySSE(
  ticketId: Ref<number | undefined>,
  onActivity: () => void,
  debounceMs = 400,
) {
  let timer: ReturnType<typeof setTimeout> | null = null

  function schedule() {
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => {
      timer = null
      onActivity()
    }, debounceMs)
  }

  // No aggregate filter: match by ticket reference instead, so any
  // ticket-scoped aggregate (including ones not enumerated here) is
  // covered. The predicate is cheap.
  useSyncActions((actions) => {
    const tid = ticketId.value
    if (tid == null) return
    if (actions.some((a) => touchesTicket(a, tid))) {
      schedule()
    }
  })

  onUnmounted(() => {
    if (timer) clearTimeout(timer)
  })
}
