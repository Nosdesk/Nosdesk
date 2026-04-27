/**
 * Ticket-backed stat widget plumbing.
 *
 * Each call subscribes to a single shared Pinia Colada cache
 * entry keyed `['tickets', 'all-for-stats']` and derives its
 * own stats from the cached set. Three widgets on the dashboard
 * (Queue, Yours, Summary) used to fire three independent
 * `getTickets()` requests; with a shared key they dedup into
 * one fetch per session and re-derive locally when SSE events
 * invalidate the cache.
 *
 * The full-list-fetch is a known scaling issue: even with one
 * shared request, deserialising every ticket just to count a
 * subset is wasteful at >5k tickets. The proper fix is a
 * `/api/tickets/stats` endpoint returning aggregated counts;
 * tracked as a backend follow-up. This composable already
 * exposes the right surface for that migration: when the
 * endpoint lands, swap the query function and the derived
 * shape, leave consumers untouched.
 */
import { computed, type ComputedRef } from 'vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import { getTickets, type Ticket } from '@/services/ticketService'
import { useSSEListeners } from '@/composables/useSSEListeners'

const TICKETS_STATS_KEY = ['tickets', 'all-for-stats'] as const
const REFETCH_COALESCE_MS = 400

// Module-level state so the SSE listener registration only fires
// once per session no matter how many `useTicketStats` callers
// mount. Without this, each consumer would attach its own SSE
// listeners and each ticket-* event would refetch N times.
let sseRegistered = false
let coalesceTimer: ReturnType<typeof setTimeout> | null = null

function registerSseInvalidation() {
  if (sseRegistered) return
  sseRegistered = true
  const queryCache = useQueryCache()
  const { on } = useSSEListeners()
  const schedule = () => {
    if (coalesceTimer) clearTimeout(coalesceTimer)
    coalesceTimer = setTimeout(() => {
      queryCache.invalidateQueries({ key: TICKETS_STATS_KEY })
      coalesceTimer = null
    }, REFETCH_COALESCE_MS)
  }
  on('ticket-updated', schedule)
  on('ticket-created', schedule)
  on('ticket-deleted', schedule)
}

export interface UseTicketStatsResult<T> {
  data: ComputedRef<T>
  loading: ComputedRef<boolean>
  error: ComputedRef<string | null>
  reload: () => Promise<unknown>
}

export function useTicketStats<T>(
  compute: (tickets: Ticket[]) => T,
  initial: T,
  errorLabel = 'Failed to load',
): UseTicketStatsResult<T> {
  registerSseInvalidation()

  const query = useQuery({
    key: TICKETS_STATS_KEY,
    query: () => getTickets(),
  })

  return {
    data: computed(() => (query.data.value ? compute(query.data.value) : initial)),
    loading: computed(() => query.asyncStatus.value === 'loading'),
    error: computed(() => (query.error.value ? errorLabel : null)),
    reload: () => query.refresh(),
  }
}
