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
 * isn't consumed directly: it primes a Pinia Colada cache
 * entry that the view's `useInfiniteQuery` reads from on mount.
 *
 * For now the loader fetches with the URL's filter set and a
 * page size that matches the user's persisted preference (or
 * 25 as the fallback when localStorage isn't available, e.g.
 * SSR). View hydration overrides this once the user lands.
 */
import { defineColadaLoader } from 'vue-router/experimental/pinia-colada'
import { setInfiniteQueryData, useQueryCache } from '@pinia/colada'
import ticketService from '@/services/ticketService'

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

function buildCacheKeyPart(query: Record<string, string | string[] | undefined>): string {
  const params: Record<string, string> = {}
  if (typeof query.search === 'string' && query.search) params.search = query.search
  for (const key of [
    'status',
    'priority',
    'category',
    'assignee',
    'requester',
    'createdOn',
    'createdAfter',
    'createdBefore',
    'modifiedOn',
    'modifiedAfter',
    'modifiedBefore',
    'closedOn',
    'closedAfter',
    'closedBefore',
  ]) {
    const value = query[key]
    if (typeof value === 'string' && value && value !== 'all') {
      params[key] = value.toLowerCase()
    }
  }
  const sortField =
    typeof query.sortField === 'string' && query.sortField ? query.sortField : 'id'
  const sortDirection =
    typeof query.sortDirection === 'string' && query.sortDirection
      ? query.sortDirection
      : 'desc'
  // Use the translated chunk size (matches `useListControls`'s
  // `effectivePageSize`) so the loader-primed key matches the
  // view's query key. Otherwise the prime is silently orphaned
  // and the view fires a fresh request on mount.
  const userPageSize = readPageSize()
  const effectivePageSize = userPageSize === 0 ? FALLBACK_INFINITE_PAGE_SIZE : userPageSize
  params.pageSize = String(effectivePageSize)
  params.sortField = sortField
  params.sortDirection = sortDirection
  return JSON.stringify(params)
}

export const useTicketsListLoader = defineColadaLoader({
  // Loader's own cache key. Distinct from the
  // `useInfiniteQuery`'s key; we use this entry as the
  // navigation lifecycle anchor and side-effect into the real
  // cache inside `query`.
  key: (to) => ['tickets', 'list-loader', JSON.stringify(to.query)],

  async query(to) {
    const cacheKey = buildCacheKeyPart(
      to.query as Record<string, string | string[] | undefined>,
    )
    const userPageSize = readPageSize()
    const isInfinite = userPageSize === 0
    const effectivePageSize = isInfinite ? FALLBACK_INFINITE_PAGE_SIZE : userPageSize

    const queryString = (key: string) =>
      typeof to.query[key] === 'string' ? (to.query[key] as string) : undefined

    const params = {
      page: 1,
      pageSize: effectivePageSize,
      sortField: queryString('sortField') ?? 'id',
      sortDirection: (queryString('sortDirection') as 'asc' | 'desc') ?? 'desc',
      search: queryString('search'),
      status: queryString('status'),
      priority: queryString('priority'),
      category: queryString('category'),
      assignee: queryString('assignee'),
      requester: queryString('requester'),
      createdAfter: queryString('createdAfter'),
      createdBefore: queryString('createdBefore'),
      createdOn: queryString('createdOn'),
      modifiedAfter: queryString('modifiedAfter'),
      modifiedBefore: queryString('modifiedBefore'),
      modifiedOn: queryString('modifiedOn'),
      closedAfter: queryString('closedAfter'),
      closedBefore: queryString('closedBefore'),
      closedOn: queryString('closedOn'),
    }

    const firstPage = await ticketService.getPaginatedTickets(
      params,
      'tickets-loader-first-page',
    )

    // Prime the matching infinite-query cache entry so the
    // view's `useInfiniteQuery` resolves from cache on mount.
    const queryCache = useQueryCache()
    const variant: 'infinite' | 'paginated' = isInfinite ? 'infinite' : 'paginated'
    const ticketsListKey = isInfinite
      ? (['tickets', 'list', variant, cacheKey] as const)
      : (['tickets', 'list', variant, cacheKey, 1] as const)

    setInfiniteQueryData(queryCache, ticketsListKey, {
      pages: [firstPage],
      pageParams: [1],
    })

    return firstPage
  },
})
