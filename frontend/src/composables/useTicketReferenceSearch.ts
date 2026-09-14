/**
 * Ticket search for the composer's `#` picker.
 *
 * The workspace pool already holds every ticket the agent can see, so
 * this is a synchronous filter over it: no request, no debounce, no
 * cancellation. A numeric query matches ticket numbers by prefix (an
 * exact number first); anything else matches the title.
 */
import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue'
import * as pool from '@nosdesk/core/sync/pool'
import { useSyncTicketsStore, type SyncTicket } from '@/sync/stores/tickets'

const DEFAULT_LIMIT = 8

export function useTicketReferenceSearch(
  query: MaybeRefOrGetter<string>,
  limit = DEFAULT_LIMIT,
): ComputedRef<SyncTicket[]> {
  const all = useSyncTicketsStore().all()
  return computed(() => {
    const q = toValue(query).trim()
    if (!q) return []
    const pool = all.value
    if (/^\d+$/.test(q)) {
      const exact = pool.find((t) => String(t.id) === q)
      const prefix = pool
        .filter((t) => t.id !== exact?.id && String(t.id).startsWith(q))
        .sort((a, b) => a.id - b.id)
      return (exact ? [exact, ...prefix] : prefix).slice(0, limit)
    }
    const needle = q.toLowerCase()
    return pool
      .filter((t) => t.title.toLowerCase().includes(needle))
      .sort((a, b) => b.last_activity_at.localeCompare(a.last_activity_at))
      .slice(0, limit)
  })
}

/**
 * True when the pool knows this ticket, so a bare `#123` can safely
 * become a link. Plain read (no reactive scope): the input rule calls it.
 */
export function poolHasTicket(id: number): boolean {
  return pool.get<SyncTicket>('ticket', id) != null
}
