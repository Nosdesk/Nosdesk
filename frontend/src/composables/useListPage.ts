/**
 * Data orchestration for a list page (tickets, users, devices, ...).
 *
 * Owns the dual-mode infinite/paginated `useInfiniteQuery` toggle,
 * the four loading-state flags (`isFirstLoad`, `isLoadingMore`,
 * `isBackgroundRefresh`, `isFetching`), SSE-driven cache
 * invalidation, mobile-search registration, infinite-scroll
 * wiring, and URL synchronisation of filters/sort/search/page.
 *
 * Pairs with `ListPageLayout.vue` (chrome) and the per-feature
 * `*Keys` factory in `@/queries`. The composable is data-shape
 * agnostic, callers supply the typed `fetchPage` function; it knows
 * nothing about Pinia Colada keys other than the shared family
 * passed in.
 *
 * Loading-state contract (must not regress, see the
 * "stop widgets flashing skeleton on remount" fix):
 *  - `isFirstLoad = isFetching && items.length === 0` (skeleton)
 *  - `isLoadingMore = isFetching && items.length > 0` (bottom spinner)
 *  - `isBackgroundRefresh = isFetching && items.length > 0` (table dim)
 *  - Empty state should be gated on `... && !isFirstLoad` so the
 *    empty-state copy doesn't flash before the first response.
 */
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  onUnmounted,
  watch,
  type ComputedRef,
  type Ref,
} from 'vue'
import { onBeforeRouteLeave, useRoute, useRouter, type LocationQuery } from 'vue-router'
import { useInfiniteQuery, useQueryCache } from '@pinia/colada'

import type { useListControls } from '@/composables/useListControls'
import { useInfiniteScroll } from '@/composables/useInfiniteScroll'
import { useMobileSearch, type CreateButtonIcon } from '@/composables/useMobileSearch'
import { useSSE } from '@/services/sseService'
import type { ListKeys } from '@/queries/listKeys'

/**
 * Per-tab scroll-position cache, keyed by route fullPath. Lost on
 * page refresh (semantics intentionally match the legacy
 * KeepAlive behaviour), preserved across in-tab navigation.
 */
const listPageScrollPositions = new Map<string, number>()

/** Shape every list endpoint must return. */
export interface ListPage<T> {
  data: T[]
  total: number
  totalPages: number
}

export interface ListPageFetchParams {
  page: number
  pageSize: number
  sortField: string
  sortDirection: 'asc' | 'desc'
  search?: string
  [filterKey: string]: string | number | undefined
}

export interface MobileSearchConfig {
  placeholder?: string
  createIcon?: CreateButtonIcon
  /** Optional: when omitted, the mobile create button is hidden. */
  onCreate?: () => void
}

export interface UrlSyncConfig {
  /** Filter keys to sync (per Q1 research, URL sync is on by
   *  default). Empty array = no filter sync; sort/search/page are
   *  always synced when `urlSync` is provided. */
  paramKeys?: readonly string[]
  /** Filter keys that should be parsed as comma-separated arrays
   *  for multi-select pickers. Subset of `paramKeys`. */
  multiSelectKeys?: readonly string[]
  /** Translate a URL value into the controls value. Used for
   *  `assignee=current` → current user uuid, etc. */
  transformValue?: (key: string, value: string) => string
}

export interface UseListPageOptions<T, R extends string> {
  controls: ReturnType<typeof useListControls<T extends Record<string, unknown> ? T : never>>
  keys: ListKeys<R>
  /** Per-feature query function. Receives the page param + the
   *  flattened params from `controls.requestParams`. */
  fetchPage: (params: ListPageFetchParams) => Promise<ListPage<T>>
  /** Scroll container that drives infinite scroll. Pass
   *  `pageScrollRef.value?.scrollContainerRef` from the layout. */
  scrollContainerRef: Ref<HTMLElement | null> | ComputedRef<HTMLElement | null>
  /** SSE event names that should invalidate this list's cache. */
  sseEvents?: readonly string[]
  /** Mobile search bar registration. Omit to skip. */
  mobileSearch?: MobileSearchConfig
  /** URL synchronisation of filters/sort/search/page. Omit for
   *  views that intentionally don't deep-link (rare; per Q1
   *  research the default is on). */
  urlSync?: UrlSyncConfig
  /** Side effect invoked with each loaded page's items (used by
   *  Devices to pre-warm the user-name cache). */
  onPageLoaded?: (items: T[]) => void | Promise<void>
  /** Optional id prefix for telemetry / network log labels. Defaults
   *  to `keys.root[0]`. */
  requestIdPrefix?: string
}

export function useListPage<T, R extends string>(
  options: UseListPageOptions<T, R>,
) {
  const { controls, keys, fetchPage, scrollContainerRef } = options
  const requestIdPrefix = options.requestIdPrefix ?? keys.root[0]
  const queryCache = useQueryCache()
  const route = useRoute()

  // ---- Dual-mode queries -----------------------------------------
  // Both queries are always wired but only one is `enabled` at a
  // time. This matches what the three views did individually and
  // lets the cache survive mode switches.
  const infiniteList = useInfiniteQuery(() => ({
    key: keys.list('infinite', controls.cacheKeyPart.value),
    initialPageParam: 1,
    query: async ({ pageParam }) => {
      const page = pageParam as number
      const response = await fetchPage({ ...controls.requestParams.value, page })
      void options.onPageLoaded?.(response.data)
      return response
    },
    getNextPageParam: (lastPage, allPages) =>
      allPages.length < lastPage.totalPages ? allPages.length + 1 : null,
    enabled: () => controls.isInfiniteMode.value,
  }))

  const paginatedList = useInfiniteQuery(() => ({
    key: keys.list('paginated', controls.cacheKeyPart.value, controls.currentPage.value),
    initialPageParam: controls.currentPage.value,
    query: async ({ pageParam }) => {
      const page = pageParam as number
      const response = await fetchPage({ ...controls.requestParams.value, page })
      void options.onPageLoaded?.(response.data)
      return response
    },
    getNextPageParam: () => null,
    enabled: () => !controls.isInfiniteMode.value,
  }))

  // ---- Derived state ---------------------------------------------
  const items = computed<T[]>(() => {
    const source = controls.isInfiniteMode.value ? infiniteList : paginatedList
    return source.data.value?.pages.flatMap((p) => p.data) ?? []
  })

  const totalItems = computed(() => {
    const source = controls.isInfiniteMode.value ? infiniteList : paginatedList
    return source.data.value?.pages[0]?.total ?? 0
  })

  const totalPages = computed(() => {
    const source = controls.isInfiniteMode.value ? infiniteList : paginatedList
    return source.data.value?.pages[0]?.totalPages ?? 1
  })

  const hasMore = computed(() =>
    controls.isInfiniteMode.value ? infiniteList.hasNextPage.value : false,
  )

  const activeAsyncStatus = computed(() =>
    controls.isInfiniteMode.value
      ? infiniteList.asyncStatus.value
      : paginatedList.asyncStatus.value,
  )

  const activeError = computed(() =>
    controls.isInfiniteMode.value ? infiniteList.error.value : paginatedList.error.value,
  )

  // ---- Loading state contract (do not regress!) -------------------
  const isFetching = computed(() => activeAsyncStatus.value === 'loading')
  const isFirstLoad = computed(() => isFetching.value && items.value.length === 0)
  const isLoadingMore = computed(() => isFetching.value && items.value.length > 0)
  const isBackgroundRefresh = computed(() => isFetching.value && items.value.length > 0)

  const errorMessage = computed(() => {
    if (!activeError.value) return null
    return activeError.value instanceof Error
      ? activeError.value.message
      : 'Failed to load. Please try again.'
  })

  function handleRetry() {
    if (controls.isInfiniteMode.value) infiniteList.refetch()
    else paginatedList.refetch()
  }

  // ---- SSE-driven cache invalidation -----------------------------
  if (options.sseEvents && options.sseEvents.length > 0) {
    const sse = useSSE()
    const invalidate = () => queryCache.invalidateQueries({ key: keys.root })
    const handlers = options.sseEvents.map((type) => ({ type, handler: invalidate }))

    onMounted(() => {
      if (!sse.isConnected.value) sse.connect()
      for (const { type, handler } of handlers) {
        sse.addEventListener(type as never, handler)
      }
    })
    onBeforeUnmount(() => {
      for (const { type, handler } of handlers) {
        sse.removeEventListener(type as never, handler)
      }
    })
  }

  // ---- Mobile search registration --------------------------------
  if (options.mobileSearch) {
    const mobileSearch = useMobileSearch()
    const cfg = options.mobileSearch
    const register = () =>
      mobileSearch.registerMobileSearch({
        searchQuery: controls.searchQuery.value,
        placeholder: cfg.placeholder,
        showCreateButton: !!cfg.onCreate,
        createIcon: cfg.createIcon,
        onSearchUpdate: controls.handleSearchUpdate,
        onCreate: cfg.onCreate,
      })

    // List views unmount on nav-away (no KeepAlive), so plain
    // mount/unmount handles register/deregister cleanly without
    // the activate/deactivate boilerplate.
    onMounted(register)
    onUnmounted(mobileSearch.deregisterMobileSearch)
    watch(controls.searchQuery, mobileSearch.updateSearchQuery)
  }

  // ---- Infinite scroll -------------------------------------------
  useInfiniteScroll({
    containerRef: scrollContainerRef as Ref<HTMLElement | null>,
    enabled: controls.isInfiniteMode,
    hasMore,
    isLoading: computed(() => isLoadingMore.value),
    onLoadMore: () => infiniteList.loadNextPage(),
  })

  // ---- Scroll restoration ----------------------------------------
  // Vue Router's built-in `scrollBehavior` only restores window
  // scroll, but our list views scroll inside a PageScroll
  // overflow:auto container. Stash the container's scrollTop on
  // route leave and restore after the next render with cached
  // data, so back-nav lands the user where they were.
  //
  // Module-scoped Map keyed by route fullPath. Survives view
  // unmount but is per-tab, lost on page refresh — matching the
  // semantics of the legacy KeepAlive scroll behaviour.
  onBeforeRouteLeave((to, from) => {
    const top = scrollContainerRef.value?.scrollTop
    if (typeof top === 'number') {
      listPageScrollPositions.set(from.fullPath, top)
    }
  })

  onMounted(async () => {
    const saved = listPageScrollPositions.get(route.fullPath)
    if (saved === undefined) return
    // Wait for the cached items (if any) to render before
    // restoring; otherwise the scroll target hasn't been laid
    // out yet and the assignment is silently clamped to 0.
    await nextTick()
    if (scrollContainerRef.value) {
      scrollContainerRef.value.scrollTop = saved
    }
  })

  // ---- URL synchronisation ---------------------------------------
  if (options.urlSync) {
    // The helper is intentionally untyped on `T`; it only reads
    // string filter values and sort/page primitives, so any
    // `useListControls<...>` instance satisfies it. The generic
    // gymnastics to thread `T` into the helper add nothing.
    setupUrlSync(options.urlSync, controls as unknown as ListControlsForUrlSync)
  }

  return {
    // Data
    items,
    totalItems,
    totalPages,
    hasMore,

    // Loading state contract
    isFetching,
    isFirstLoad,
    isLoadingMore,
    isBackgroundRefresh,

    // Error
    errorMessage,
    handleRetry,

    // Underlying queries (escape hatch for refetches / cache pokes)
    infiniteList,
    paginatedList,

    // Identity for diagnostics
    requestIdPrefix,
  }
}

// ----------------------------------------------------------------------
// URL sync internals.
//
// Default behaviour (per Q1 research): on. Filters/sort/search/page
// are pushed to the URL so refresh-survives, deep-links work, and
// shareable URLs are first-class. Discrete user choices use
// `pushState` (back button works); the search input uses
// `replaceState` (debouncing happens in DebouncedSearchInput, the
// store doesn't need to debounce again).
//
// Empty / default values are stripped from the URL so the canonical
// form stays short.
// ----------------------------------------------------------------------

/** Subset of `useListControls`'s return shape that the URL-sync
 *  helper actually touches. Avoids leaking the controls' generic
 *  parameter through the helper, which has no semantic dependency
 *  on the item type. */
interface ListControlsForUrlSync {
  filters: Ref<Record<string, string | string[]>>
  searchQuery: Ref<string>
  sortField: Ref<string>
  sortDirection: Ref<'asc' | 'desc'>
  currentPage: Ref<number>
}

function setupUrlSync(
  config: UrlSyncConfig,
  controls: ListControlsForUrlSync,
): void {
  const route = useRoute()
  const router = useRouter()
  const paramKeys = config.paramKeys ?? []
  const multiSelectKeys = new Set(config.multiSelectKeys ?? [])

  function parseUrlFilters(query: LocationQuery): Record<string, string | string[]> {
    const filters: Record<string, string | string[]> = {}
    for (const key of paramKeys) {
      const raw = query[key]
      if (typeof raw !== 'string' || !raw) continue
      const transformed = config.transformValue ? config.transformValue(key, raw) : raw
      if (multiSelectKeys.has(key) && transformed.includes(',')) {
        filters[key] = transformed.split(',')
      } else if (multiSelectKeys.has(key)) {
        filters[key] = [transformed]
      } else {
        filters[key] = transformed
      }
    }
    return filters
  }

  function applyFromUrl() {
    const newFilters = parseUrlFilters(route.query)
    const newSearch =
      typeof route.query.search === 'string' ? route.query.search : ''
    const newSortField =
      typeof route.query.sortField === 'string' ? route.query.sortField : controls.sortField.value
    const newSortDirection =
      route.query.sortDirection === 'asc' || route.query.sortDirection === 'desc'
        ? route.query.sortDirection
        : controls.sortDirection.value
    const newPage =
      typeof route.query.page === 'string' && Number.isFinite(parseInt(route.query.page))
        ? parseInt(route.query.page)
        : 1

    if (JSON.stringify(newFilters) !== JSON.stringify(controls.filters.value)) {
      controls.filters.value = newFilters
    }
    if (newSearch !== controls.searchQuery.value) controls.searchQuery.value = newSearch
    if (newSortField !== controls.sortField.value) controls.sortField.value = newSortField
    if (newSortDirection !== controls.sortDirection.value) {
      controls.sortDirection.value = newSortDirection
    }
    if (newPage !== controls.currentPage.value) controls.currentPage.value = newPage
  }

  // First mount: pull URL state into controls before the query
  // fires. After that, a normal route.query watcher handles
  // in-route URL changes (e.g. dashboard tile click adding
  // `?status=open` while we're already on `/tickets`). The
  // KeepAlive race that previously required `onBeforeRouteUpdate`
  // + path gating is gone now that list views unmount on nav-away,
  // there's no cached sibling to mirror the wrong URL into.
  applyFromUrl()
  watch(() => route.query, applyFromUrl, { deep: true })

  // Controls → URL. We push for discrete state (filters/sort/page)
  // and replace for free-form text (search). Avoid pushing when
  // the URL already matches to prevent history pollution from
  // round-trips.
  function buildQuery(replace: boolean): LocationQuery {
    const q: LocationQuery = { ...route.query }

    // Filters
    for (const key of paramKeys) {
      const v = controls.filters.value[key]
      if (Array.isArray(v) && v.length > 0) q[key] = v.join(',')
      else if (typeof v === 'string' && v && v !== 'all') q[key] = v
      else delete q[key]
    }

    // Search
    if (controls.searchQuery.value) q.search = controls.searchQuery.value
    else delete q.search

    // Sort: only persist non-default values
    if (controls.sortField.value && controls.sortField.value !== 'id') {
      q.sortField = controls.sortField.value
    } else {
      delete q.sortField
    }
    if (controls.sortDirection.value && controls.sortDirection.value !== 'desc') {
      q.sortDirection = controls.sortDirection.value
    } else {
      delete q.sortDirection
    }

    // Page: only when > 1
    if (controls.currentPage.value > 1) {
      q.page = String(controls.currentPage.value)
    } else {
      delete q.page
    }

    void replace
    return q
  }

  function pushControlsToUrl(replace: boolean) {
    const query = buildQuery(replace)
    if (JSON.stringify(query) === JSON.stringify(route.query)) return
    void (replace
      ? router.replace({ query })
      : router.push({ query }))
  }

  // Discrete user choices: push (back button undoes them).
  watch(
    [
      () => controls.filters.value,
      () => controls.sortField.value,
      () => controls.sortDirection.value,
      () => controls.currentPage.value,
    ],
    () => pushControlsToUrl(false),
    { deep: true },
  )

  // Search: replace (avoids history pollution per keystroke; the
  // DebouncedSearchInput already throttles the value updates).
  watch(
    () => controls.searchQuery.value,
    () => pushControlsToUrl(true),
  )
}
