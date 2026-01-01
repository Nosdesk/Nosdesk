import { shallowRef, ref, computed, onMounted, onActivated, onDeactivated, onUnmounted, triggerRef, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useMobileSearch, type CreateButtonIcon } from './useMobileSearch'
import type { PaginationParams } from '@/types/pagination'

interface ListOptions<T> {
  itemIdField?: string
  defaultPageSize?: number
  defaultSortField?: string
  defaultSortDirection?: 'asc' | 'desc'
  fetchFunction: (params: PaginationParams) => Promise<{ data: T[], total: number, totalPages: number }>
  routeBuilder?: (item: T) => string
  // Mobile search bar options
  mobileSearch?: {
    placeholder?: string
    showCreateButton?: boolean
    createIcon?: CreateButtonIcon
    onCreate?: () => void
  }
}

export function useListManagement<T extends Record<string, unknown>>(options: ListOptions<T>) {
  const router = useRouter()
  const itemIdField = options.itemIdField || 'id'

  // Use shallowRef for items - avoids deep reactivity overhead
  const items = shallowRef<T[]>([])
  const loading = ref(false)
  const loadingMore = ref(false)
  const error = ref<string | null>(null)

  // Pagination (pageSize 0 = infinite scroll mode)
  const currentPage = ref(1)
  const pageSize = ref(options.defaultPageSize || 0)
  const pageSizeOptions = [25, 50, 100, 0] // 0 = All (infinite scroll)
  const totalItems = ref(0)
  const totalPages = ref(1)
  const hasMore = computed(() => currentPage.value < totalPages.value)

  // Infinite scroll mode
  const isInfiniteMode = computed(() => pageSize.value === 0)

  // Sorting
  const sortField = ref(options.defaultSortField || 'id')
  const sortDirection = ref<'asc' | 'desc'>(options.defaultSortDirection || 'asc')

  // Search and filters
  const searchQuery = ref('')
  const filters = ref<Record<string, string | string[]>>({})

  // Selection
  const selectedItems = ref<string[]>([])
  const lastSelectedItemId = ref<string | null>(null)

  // Loading guard
  let loadPromise: Promise<void> | null = null

  // Track background refresh (sort/filter change with existing data)
  const isBackgroundRefresh = ref(false)

  /**
   * Core fetch function - designed for infinite scroll first.
   * @param append - If true, append results to existing items (infinite scroll)
   *                 If false, replace items (pagination or refresh)
   */
  const fetchPage = async (page: number, append: boolean = false): Promise<void> => {
    if (loadPromise) return loadPromise

    const hasExistingItems = items.value.length > 0

    // Set appropriate loading state
    if (append) {
      loadingMore.value = true
    } else if (!hasExistingItems) {
      loading.value = true
    } else {
      isBackgroundRefresh.value = true
    }
    error.value = null

    loadPromise = (async () => {
      try {
        const normalizedFilters = Object.fromEntries(
          Object.entries(filters.value)
            .filter(([_, v]) => {
              // Filter out 'all' and empty arrays
              if (Array.isArray(v)) return v.length > 0
              return v !== 'all' && v !== ''
            })
            .map(([k, v]) => {
              // Convert arrays to comma-separated strings for API
              if (Array.isArray(v)) {
                return [k, v.map(val => val.toLowerCase()).join(',')]
              }
              return [k, v.toLowerCase()]
            })
        )

        // In infinite mode, use a reasonable page size for chunked loading
        const effectivePageSize = isInfiniteMode.value ? 50 : pageSize.value

        const response = await options.fetchFunction({
          page,
          pageSize: effectivePageSize,
          sortField: sortField.value,
          sortDirection: sortDirection.value,
          search: searchQuery.value,
          ...normalizedFilters
        })

        // Update current page
        currentPage.value = page

        // Append or replace items
        if (append) {
          items.value = [...items.value, ...response.data]
        } else {
          items.value = response.data
        }

        totalItems.value = response.total
        totalPages.value = response.totalPages
      } catch (err) {
        console.error('Failed to fetch items:', err)
        error.value = 'Failed to load items. Please try again later.'
      } finally {
        loading.value = false
        loadingMore.value = false
        isBackgroundRefresh.value = false
        loadPromise = null
      }
    })()

    return loadPromise
  }

  // Convenience wrappers
  const fetchItems = () => fetchPage(1, false)

  const loadMore = async (): Promise<void> => {
    if (!hasMore.value || loadPromise) return
    await fetchPage(currentPage.value + 1, true)
  }

  const refresh = async (): Promise<void> => {
    await fetchPage(1, false)
  }

  // Filter UI helper
  const buildFilterOptions = (configs: Record<string, {
    options: Array<{ value: string, label: string }>
    width?: string
    allLabel?: string
    placeholder?: string
    multiple?: boolean
  }>) => Object.entries(configs).map(([name, config]) => ({
    name,
    value: config.multiple
      ? (Array.isArray(filters.value[name]) ? filters.value[name] : [])
      : (filters.value[name] || 'all'),
    options: [
      { value: 'all', label: config.allLabel || `All ${name.charAt(0).toUpperCase() + name.slice(1)}` },
      ...config.options
    ],
    width: config.width || 'w-[120px]',
    placeholder: config.placeholder || config.allLabel || `All ${name.charAt(0).toUpperCase() + name.slice(1)}`,
    multiple: config.multiple || false
  }))

  // Handlers
  const handleFilterUpdate = (name: string, value: string | string[]) => {
    filters.value[name] = value
    refresh()
  }

  const handleSearchUpdate = (value: string) => {
    searchQuery.value = value
    refresh()
  }

  const handleSortUpdate = (field: string, direction: 'asc' | 'desc') => {
    sortField.value = field
    sortDirection.value = direction
    refresh()
  }

  // For pagination mode: jump to specific page (replaces items)
  const handlePageChange = (page: number) => {
    fetchPage(page, false)
  }

  const handlePageSizeChange = (size: number) => {
    pageSize.value = size
    refresh()
  }

  const resetFilters = () => {
    searchQuery.value = ''
    filters.value = {}
    refresh()
  }

  // Selection
  const toggleSelection = (event: Event, itemId: string) => {
    event.stopPropagation()
    const idx = selectedItems.value.indexOf(itemId)

    if (event instanceof MouseEvent && event.shiftKey && lastSelectedItemId.value) {
      const curIdx = items.value.findIndex(i => i[itemIdField].toString() === itemId)
      const lastIdx = items.value.findIndex(i => i[itemIdField].toString() === lastSelectedItemId.value)
      if (curIdx !== -1 && lastIdx !== -1) {
        const [start, end] = curIdx < lastIdx ? [curIdx, lastIdx] : [lastIdx, curIdx]
        items.value.slice(start, end + 1).forEach(item => {
          const id = item[itemIdField].toString()
          if (!selectedItems.value.includes(id)) selectedItems.value.push(id)
        })
      }
    } else {
      idx === -1 ? selectedItems.value.push(itemId) : selectedItems.value.splice(idx, 1)
      lastSelectedItemId.value = itemId
    }
  }

  const toggleAllItems = (event: Event) => {
    event.stopPropagation()
    // Check if all items are currently selected
    const allSelected = items.value.length > 0 &&
      items.value.every(i => selectedItems.value.includes(i[itemIdField].toString()))
    // Toggle: if all selected, clear; otherwise select all
    selectedItems.value = allSelected ? [] : items.value.map(i => i[itemIdField].toString())
    lastSelectedItemId.value = null
  }

  const navigateToItem = (item: T) => {
    if (options.routeBuilder) router.push(options.routeBuilder(item))
  }

  // Bulk selection helpers
  const clearSelection = () => {
    selectedItems.value = []
    lastSelectedItemId.value = null
  }

  const selectAll = () => {
    selectedItems.value = items.value.map(i => i[itemIdField].toString())
  }

  // Item mutation methods for SSE real-time updates
  const getItem = (id: string | number): T | undefined => {
    return items.value.find(item => item[itemIdField].toString() === id.toString())
  }

  const hasItem = (id: string | number): boolean => {
    return items.value.some(item => item[itemIdField].toString() === id.toString())
  }

  const updateItemField = <K extends keyof T>(id: string | number, field: K, value: T[K]): boolean => {
    const item = getItem(id)
    if (!item) return false
    // Cast to allow mutation of the item field
    ;(item as Record<K, T[K]>)[field] = value
    triggerRef(items)
    return true
  }

  const removeItem = (id: string | number): boolean => {
    const index = items.value.findIndex(item => item[itemIdField].toString() === id.toString())
    if (index === -1) return false
    items.value.splice(index, 1)
    totalItems.value = Math.max(0, totalItems.value - 1)
    triggerRef(items)
    return true
  }

  const prependItem = (item: T): void => {
    items.value.unshift(item)
    totalItems.value += 1
    triggerRef(items)
  }

  onMounted(fetchItems)

  // Refresh on reactivation (KeepAlive) to catch changes while cached
  onActivated(() => {
    if (items.value.length > 0) {
      refresh()
    }
  })

  // Mobile search bar integration
  if (options.mobileSearch) {
    const { registerMobileSearch, deregisterMobileSearch, updateSearchQuery } = useMobileSearch()

    const setupMobileSearch = () => {
      registerMobileSearch({
        searchQuery: searchQuery.value,
        placeholder: options.mobileSearch!.placeholder || 'Search...',
        showCreateButton: options.mobileSearch!.showCreateButton ?? true,
        createIcon: options.mobileSearch!.createIcon,
        onSearchUpdate: handleSearchUpdate,
        onCreate: options.mobileSearch!.onCreate
      })
    }

    onMounted(setupMobileSearch)
    onActivated(setupMobileSearch)
    onDeactivated(deregisterMobileSearch)
    onUnmounted(deregisterMobileSearch)

    // Sync search query changes to mobile search bar
    watch(searchQuery, updateSearchQuery)
  }

  return {
    // State
    items, loading, loadingMore, error, isBackgroundRefresh,
    currentPage, pageSize, pageSizeOptions, totalItems, totalPages, hasMore,
    isInfiniteMode,
    sortField, sortDirection,
    searchQuery, filters,
    selectedItems,

    // Actions
    fetchItems, loadMore, refresh,
    handleFilterUpdate, handleSearchUpdate, handleSortUpdate,
    handlePageChange, handlePageSizeChange, resetFilters,
    toggleSelection, toggleAllItems, navigateToItem,
    clearSelection, selectAll,
    buildFilterOptions,

    // Item mutation methods for SSE
    getItem, hasItem, updateItemField, removeItem, prependItem,
  }
}
