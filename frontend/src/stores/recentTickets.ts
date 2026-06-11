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
import { computed, ref, watch } from 'vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import ticketService from '@/services/ticketService'
import type { RecentTicket } from '@/types/ticket'
import { logger } from '@/utils/logger'
import { translate } from '@/i18n'
import { useAuthStore } from '@/stores/auth'

export const RECENT_TICKETS_KEY = ['tickets', 'recent'] as const

// Cap on how many rows we persist. The list never shows more than a
// handful, so a small cap keeps the localStorage payload tiny.
const MAX_PERSISTED = 25
const STORAGE_KEY_PREFIX = 'nosdesk:recent-tickets'

/** Per-account storage key so switching accounts on the same browser
 *  doesn't hydrate the previous account's list. */
function storageKey(accountUuid: string | null): string {
  return accountUuid
    ? `${STORAGE_KEY_PREFIX}:${accountUuid}`
    : `${STORAGE_KEY_PREFIX}:anon`
}

function loadFromStorage(accountUuid: string | null): RecentTicket[] {
  if (typeof localStorage === 'undefined') return []
  try {
    const raw = localStorage.getItem(storageKey(accountUuid))
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? (parsed as RecentTicket[]) : []
  } catch {
    return []
  }
}

export const useRecentTicketsStore = defineStore('recentTickets', () => {
  const queryCache = useQueryCache()
  const auth = useAuthStore()
  const accountKey = () => auth.user?.uuid ?? null

  // Seed the Colada cache from localStorage *before* the query is
  // created so a hard refresh renders the last-known list instantly:
  // `query.data` is already defined, `isLoading` stays false, and the
  // sidebar never flashes a skeleton or jumps height. The query below
  // attaches to this same cache entry and refetches in the background
  // to bring it up to date.
  const seeded = loadFromStorage(accountKey())
  if (seeded.length > 0) {
    queryCache.setQueryData<RecentTicket[]>(RECENT_TICKETS_KEY, seeded)
  }

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

  // Persist the server list so the next page load can hydrate from it
  // instantly. We persist server order (not the local drag override),
  // since that override resets on the next refetch anyway.
  watch(baseTickets, (next) => {
    if (typeof localStorage === 'undefined') return
    try {
      localStorage.setItem(
        storageKey(accountKey()),
        JSON.stringify(next.slice(0, MAX_PERSISTED)),
      )
    } catch {
      // Quota exceeded or storage disabled. Best-effort; the in-memory
      // cache still drives the UI for this session.
    }
  })

  // Re-hydrate and refetch whenever the signed-in account changes so we
  // never show one account's recents to another. Seeding with the new
  // account's list (or [] when it has none) first clears any stale rows.
  watch(
    () => accountKey(),
    (uuid) => {
      queryCache.setQueryData<RecentTicket[]>(RECENT_TICKETS_KEY, loadFromStorage(uuid))
      queryCache.invalidateQueries({ key: RECENT_TICKETS_KEY })
    },
  )

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
  const error = computed(() =>
    query.error.value
      ? translate('error-store-recent-tickets-load', undefined, 'Failed to fetch recent tickets')
      : null,
  )

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
    const current = recentTickets.value
    if (fromIndex < 0 || fromIndex >= current.length) return
    const next = [...current]
    const [moved] = next.splice(fromIndex, 1)
    const clamped = Math.max(0, Math.min(toIndex, next.length))
    if (fromIndex === clamped) return
    next.splice(clamped, 0, moved)
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
