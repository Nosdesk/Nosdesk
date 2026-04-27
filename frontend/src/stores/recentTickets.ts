/**
 * Recent-tickets store. Backed by Pinia Colada (`useQuery`) so
 * the sidebar (`<RecentTickets>`) and the dashboard widget
 * (`<RecentlyViewedWidget>`) share one cache entry, keyed by
 * `['tickets', 'recent']`. Both consumers see the same data and
 * a mutation here flows through to both surfaces immediately.
 *
 * Local UI overrides (removed-ids suppression, drag-reorder)
 * stay in store-local refs because they're per-session
 * client-only, not server-authoritative state.
 */
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import ticketService from '@/services/ticketService'
import type { RecentTicket } from '@/types/ticket'
import { logger } from '@/utils/logger'

export const RECENT_TICKETS_KEY = ['tickets', 'recent'] as const

export const useRecentTicketsStore = defineStore('recentTickets', () => {
  const queryCache = useQueryCache()

  // Recently-removed ids stay suppressed until the next refetch
  // so a quick `recordTicketView` after `removeTicket` doesn't
  // immediately re-add the just-dismissed entry.
  const removedTicketIds = ref<Set<number>>(new Set())

  // Local drag-to-reorder override. Server returns most-recent
  // first; if the user drags rows we honour that local order
  // until the next refetch, when the server order resumes.
  const orderOverride = ref<number[] | null>(null)

  const query = useQuery({
    key: RECENT_TICKETS_KEY,
    query: () => ticketService.getRecentTickets(),
  })

  const baseTickets = computed<RecentTicket[]>(() => query.data.value ?? [])

  const recentTickets = computed<RecentTicket[]>(() => {
    let list = removedTicketIds.value.size > 0
      ? baseTickets.value.filter((t) => !removedTicketIds.value.has(t.id))
      : baseTickets.value
    if (orderOverride.value) {
      const order = orderOverride.value
      const byId = new Map(list.map((t) => [t.id, t]))
      const ordered: RecentTicket[] = []
      for (const id of order) {
        const item = byId.get(id)
        if (item) {
          ordered.push(item)
          byId.delete(id)
        }
      }
      // Append any tickets that arrived after the reorder.
      for (const item of byId.values()) ordered.push(item)
      list = ordered
    }
    return list
  })

  // `isLoading` is the *first-fetch* signal (no cached data yet), so
  // consumers can render a skeleton on initial load without flashing
  // it on every dashboard remount, when Pinia Colada serves cached
  // data and fires a background refetch in parallel. `isRefreshing`
  // covers that background-refetch case for callers that want to
  // surface it (e.g. a top-of-card shimmer).
  const isLoading = computed(
    () => query.status.value === 'pending' && query.data.value === undefined,
  )
  const isRefreshing = computed(
    () => query.asyncStatus.value === 'loading' && query.data.value !== undefined,
  )
  const error = computed(() => (query.error.value ? 'Failed to fetch recent tickets' : null))

  function fetchRecentTickets() {
    return query.refresh()
  }

  async function recordTicketView(ticketId: number) {
    if (removedTicketIds.value.has(ticketId)) return
    try {
      await ticketService.recordTicketView(ticketId)
      // Invalidate so Colada refetches and the new entry slots
      // in at the top with correct ordering.
      queryCache.invalidateQueries({ key: RECENT_TICKETS_KEY })
    } catch (err) {
      logger.error(`Error recording view for ticket #${ticketId}:`, err)
    }
  }

  function updateTicketData(ticketId: number, updatedData: Partial<RecentTicket>) {
    queryCache.setQueryData<RecentTicket[]>(RECENT_TICKETS_KEY, (old) => {
      if (!old) return old as never
      return old.map((t) => (t.id === ticketId ? { ...t, ...updatedData } : t))
    })
  }

  async function removeTicket(ticketId: number) {
    removedTicketIds.value.add(ticketId)
    // Optimistic remove from cache so both consumers update.
    queryCache.setQueryData<RecentTicket[]>(RECENT_TICKETS_KEY, (old) =>
      (old ?? []).filter((t) => t.id !== ticketId),
    )
    try {
      await ticketService.removeRecentTicket(ticketId)
      removedTicketIds.value.delete(ticketId)
    } catch (err) {
      logger.error(`Error removing ticket #${ticketId} from recent:`, err)
      removedTicketIds.value.delete(ticketId)
      // Server rejected the delete; refetch to restore truth.
      queryCache.invalidateQueries({ key: RECENT_TICKETS_KEY })
    }
  }

  function reorderTickets(fromIndex: number, toIndex: number) {
    if (fromIndex === toIndex) return
    const current = recentTickets.value
    if (fromIndex < 0 || fromIndex >= current.length) return
    if (toIndex < 0 || toIndex >= current.length) return
    const next = [...current]
    const [moved] = next.splice(fromIndex, 1)
    next.splice(toIndex, 0, moved)
    orderOverride.value = next.map((t) => t.id)
  }

  return {
    recentTickets,
    isLoading,
    isRefreshing,
    error,
    fetchRecentTickets,
    recordTicketView,
    updateTicketData,
    removeTicket,
    reorderTickets,
  }
})
