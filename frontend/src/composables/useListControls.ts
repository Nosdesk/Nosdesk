/**
 * UI-state composable for list views (filters, sort, search,
 * selection, page size). No data-fetching side effects: state
 * changes here are picked up by the data layer's reactive query
 * key, which Pinia Colada uses to refetch automatically.
 *
 * Pairs with `useInfiniteQuery({ key: () => [...controls.params] })`
 * in the consuming view. Selection is local; the data composable
 * doesn't need to know about it.
 *
 * Replaces the UI-state half of the legacy `useListManagement`
 * composable. The data half lives in Pinia Colada now.
 */
import { computed, ref, type Ref } from 'vue'
import { serializeListCacheKey, type ListRequestParams } from '@/queries/listSerialization'

export type FilterValue = string | string[]
export type Filters = Record<string, FilterValue>
export type SortDirection = 'asc' | 'desc'

export interface ListControlsOptions {
  /** Field used to derive item id strings for selection. */
  itemIdField?: string
  defaultSortField?: string
  defaultSortDirection?: SortDirection
  /** 0 = infinite scroll mode, otherwise items per page. */
  defaultPageSize?: number
  /** Chunk size to send to the backend when in infinite mode.
   *  The UI's `pageSize === 0` is a "load all / infinite" sentinel
   *  the backend doesn't understand; we translate it here so the
   *  request always carries a real number. Default 50 matches the
   *  legacy `useListManagement` behaviour. */
  infinitePageSize?: number
  /** Initial values (typically read from URL params). */
  initialSearch?: string
  initialFilters?: Filters
  initialPage?: number
  initialPageSize?: number
}

export interface FilterOptionConfig {
  options: Array<{ value: string; label: string }>
  width?: string
  allLabel?: string
  placeholder?: string
  multiple?: boolean
}

export interface BuiltFilterOption {
  name: string
  value: FilterValue
  options: Array<{ value: string; label: string }>
  width: string
  placeholder: string
  multiple: boolean
}

const DEFAULT_PAGE_SIZE_OPTIONS = [25, 50, 100, 0] as const

export function useListControls<T extends Record<string, unknown>>(
  options: ListControlsOptions = {},
) {
  const itemIdField = options.itemIdField ?? 'id'
  const infinitePageSize = options.infinitePageSize ?? 50

  // ---- Filters / search / sort ------------------------------
  const searchQuery = ref(options.initialSearch ?? '')
  const filters = ref<Filters>(options.initialFilters ?? {})
  const sortField = ref(options.defaultSortField ?? 'id')
  const sortDirection = ref<SortDirection>(options.defaultSortDirection ?? 'asc')

  // ---- Pagination -------------------------------------------
  const currentPage = ref(options.initialPage ?? 1)
  const pageSize = ref(options.initialPageSize ?? options.defaultPageSize ?? 0)
  const pageSizeOptions = DEFAULT_PAGE_SIZE_OPTIONS
  const isInfiniteMode = computed(() => pageSize.value === 0)

  // ---- Selection (shift-click range supported) --------------
  const selectedItems = ref<string[]>([])
  const lastSelectedItemId = ref<string | null>(null)

  function toggleSelection(event: Event, itemId: string, items: readonly T[]) {
    event.stopPropagation()
    if (
      event instanceof MouseEvent &&
      event.shiftKey &&
      lastSelectedItemId.value !== null
    ) {
      const curIdx = items.findIndex((i) => String(i[itemIdField]) === itemId)
      const lastIdx = items.findIndex(
        (i) => String(i[itemIdField]) === lastSelectedItemId.value,
      )
      if (curIdx !== -1 && lastIdx !== -1) {
        const [start, end] = curIdx < lastIdx ? [curIdx, lastIdx] : [lastIdx, curIdx]
        const next = new Set(selectedItems.value)
        for (let i = start; i <= end; i++) next.add(String(items[i][itemIdField]))
        selectedItems.value = Array.from(next)
      }
      return
    }
    const idx = selectedItems.value.indexOf(itemId)
    if (idx === -1) selectedItems.value.push(itemId)
    else selectedItems.value.splice(idx, 1)
    lastSelectedItemId.value = itemId
  }

  function toggleAllItems(event: Event, items: readonly T[]) {
    event.stopPropagation()
    const allIds = items.map((i) => String(i[itemIdField]))
    const allSelected =
      allIds.length > 0 && allIds.every((id) => selectedItems.value.includes(id))
    selectedItems.value = allSelected ? [] : allIds
    lastSelectedItemId.value = null
  }

  function clearSelection() {
    selectedItems.value = []
    lastSelectedItemId.value = null
  }

  function selectAll(items: readonly T[]) {
    selectedItems.value = items.map((i) => String(i[itemIdField]))
  }

  // ---- Filter UI helpers ------------------------------------

  function buildFilterOptions(
    configs: Record<string, FilterOptionConfig>,
  ): BuiltFilterOption[] {
    return Object.entries(configs).map(([name, config]) => ({
      name,
      value: config.multiple
        ? Array.isArray(filters.value[name])
          ? filters.value[name]
          : []
        : filters.value[name] ?? 'all',
      options: [
        {
          value: 'all',
          label:
            config.allLabel ??
            `All ${name.charAt(0).toUpperCase()}${name.slice(1)}`,
        },
        ...config.options,
      ],
      width: config.width ?? 'w-[120px]',
      placeholder:
        config.placeholder ??
        config.allLabel ??
        `All ${name.charAt(0).toUpperCase()}${name.slice(1)}`,
      multiple: config.multiple ?? false,
    }))
  }

  function handleFilterUpdate(name: string, value: FilterValue) {
    filters.value[name] = value
    currentPage.value = 1
  }

  function handleSearchUpdate(value: string) {
    searchQuery.value = value
    currentPage.value = 1
  }

  function handleSortUpdate(field: string, direction: SortDirection) {
    sortField.value = field
    sortDirection.value = direction
    currentPage.value = 1
  }

  function handlePageChange(page: number) {
    currentPage.value = page
  }

  function handlePageSizeChange(size: number) {
    pageSize.value = size
    currentPage.value = 1
  }

  function resetFilters() {
    searchQuery.value = ''
    filters.value = {}
    currentPage.value = 1
  }

  /** Normalised request params suitable for a fetcher. Filters
   *  are flattened (arrays joined as comma-separated, "all" /
   *  empty values dropped) so the API sees only meaningful
   *  filter values. `pageSize` is always a real fetch chunk
   *  size: the UI's `0` ("All / infinite") sentinel is
   *  translated to `infinitePageSize` here so backends never
   *  see it. */
  const effectivePageSize = computed(() =>
    pageSize.value === 0 ? infinitePageSize : pageSize.value,
  )

  const requestParams = computed<ListRequestParams>(() => {
    const normalisedFilters: Record<string, string> = {}
    for (const [k, v] of Object.entries(filters.value)) {
      if (Array.isArray(v)) {
        if (v.length > 0) normalisedFilters[k] = v.map((x) => x.toLowerCase()).join(',')
      } else if (v !== '' && v !== 'all') {
        normalisedFilters[k] = v.toLowerCase()
      }
    }
    return {
      page: currentPage.value,
      pageSize: effectivePageSize.value,
      sortField: sortField.value,
      sortDirection: sortDirection.value,
      search: searchQuery.value,
      ...normalisedFilters,
    }
  })

  /** Stable serialisation of `requestParams` for use as a Pinia
   *  Colada reactive query key. Delegates to `serializeListCacheKey`
   *  so loaders (which build the same key from `to.query` outside
   *  the Vue tree) can call into the same function and stay in
   *  sync. */
  const cacheKeyPart = computed(() => serializeListCacheKey(requestParams.value))

  return {
    // Filter / search / sort state
    searchQuery,
    filters: filters as Ref<Filters>,
    sortField,
    sortDirection,

    // Pagination state
    currentPage,
    pageSize,
    pageSizeOptions,
    isInfiniteMode,

    // Selection state
    selectedItems,

    // Derived
    requestParams,
    cacheKeyPart,

    // Handlers
    handleFilterUpdate,
    handleSearchUpdate,
    handleSortUpdate,
    handlePageChange,
    handlePageSizeChange,
    resetFilters,
    toggleSelection,
    toggleAllItems,
    clearSelection,
    selectAll,
    buildFilterOptions,
  }
}
