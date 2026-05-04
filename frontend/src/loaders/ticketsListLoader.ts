/**
 * Vue Router Data Loader for the /tickets route.
 *
 * Runs DURING navigation to /tickets, so the first page of
 * tickets is in flight before the route component code finishes
 * downloading. Mirrors the pattern in `inboxLoader.ts`.
 *
 * Reads the same URL query params the view uses to determine
 * filters / sort / pagination so the loader's network request
 * matches what the view will subscribe to. The loader's data
 * isn't consumed directly: it primes a Pinia Colada cache entry
 * that the view's `useInfiniteQuery` reads from on mount.
 *
 * Cache-key construction routes through the shared
 * `ticketsKeys.list(...)` + `serializeListCacheKey(...)` helpers
 * (in `@/queries/tickets`, `@/queries/listSerialization`) so the
 * loader's prime hits the same key the view's first request
 * produces. Drift here used to silently orphan the prime.
 */
import { defineColadaLoader } from 'vue-router/experimental/pinia-colada'
import { setInfiniteQueryData, useQueryCache } from '@pinia/colada'
import ticketService from '@/services/ticketService'
import { savedViewsService } from '@/services/savedViewsService'
import { useSavedViewsStore } from '@/stores/savedViews'
import { ticketsKeys, ticketsListParamsFromQuery } from '@/queries/tickets'
import { serializeListCacheKey } from '@/queries/listSerialization'

const PAGESIZE_STORAGE_KEY = 'tickets-page-size'
const DEFAULT_PAGE_SIZE = 25
// Matches `useListControls`'s default `infinitePageSize`. Both
// the loader (running outside Vue) and the view-side composable
// must agree on the chunk size translated from the UI's "All"
// sentinel (pageSize === 0) so loader-primed cache entries hit
// the same key the view's query produces.
const FALLBACK_INFINITE_PAGE_SIZE = 50

function readPageSize(): number {
  if (typeof localStorage === 'undefined') return DEFAULT_PAGE_SIZE
  const saved = localStorage.getItem(PAGESIZE_STORAGE_KEY)
  if (saved === null) return 0
  const parsed = parseInt(saved)
  return Number.isFinite(parsed) ? parsed : DEFAULT_PAGE_SIZE
}

export const useTicketsListLoader = defineColadaLoader({
  // Loader's own cache key. Distinct from the view's
  // `useInfiniteQuery` key; we use this entry as the navigation
  // lifecycle anchor and side-effect into the real cache inside
  // `query`.
  key: (to) => ['tickets', 'list-loader', JSON.stringify(to.query)],

  async query(to) {
    const userPageSize = readPageSize()
    const isInfinite = userPageSize === 0
    const effectivePageSize = isInfinite ? FALLBACK_INFINITE_PAGE_SIZE : userPageSize

    // Build the same params shape the view's `requestParams`
    // produces, then derive the cache-key string and the fetch
    // params from the same source of truth.
    const params = ticketsListParamsFromQuery(to.query, { pageSize: effectivePageSize })
    const cacheKey = serializeListCacheKey(params)

    // Fetch the first page of tickets and the workspace-scoped
    // saved-view list in parallel. Both are needed before the
    // view-resolution computed in TicketsListView can decide
    // which view to render; loading them sequentially is what
    // produced the My Queue → Triage flash on first mount.
    const savedViewsStore = useSavedViewsStore()
    const [firstPage, savedViewRows] = await Promise.all([
      ticketService.getPaginatedTickets(params, 'tickets-loader-first-page'),
      savedViewsService.list().catch(() => []),
    ])
    savedViewsStore.prime(null, savedViewRows)

    // Prime the matching infinite-query cache entry so the view's
    // `useInfiniteQuery` resolves from cache on mount.
    const queryCache = useQueryCache()
    const variant = isInfinite ? 'infinite' : 'paginated'
    const ticketsListKey = isInfinite
      ? ticketsKeys.list(variant, cacheKey)
      : ticketsKeys.list(variant, cacheKey, 1)

    setInfiniteQueryData(queryCache, ticketsListKey, {
      pages: [firstPage],
      pageParams: [1],
    })

    return firstPage
  },
})
