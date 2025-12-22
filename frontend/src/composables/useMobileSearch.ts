import { ref, readonly } from 'vue'

// Global state for mobile search bar
const searchQuery = ref('')
const placeholder = ref('Search...')
const showCreateButton = ref(true)
const createButtonLoading = ref(false)
const isActive = ref(false)

// Callbacks registered by the current view
let onSearchUpdate: ((value: string) => void) | null = null
let onCreate: (() => void) | null = null

/**
 * Composable for mobile search bar state management.
 * List views call registerMobileSearch to connect their search to the mobile bar.
 */
export function useMobileSearch() {
  /**
   * Register the current view's search handlers.
   * Call this in onMounted/onActivated, and deregister in onDeactivated.
   */
  const registerMobileSearch = (options: {
    searchQuery: string
    placeholder?: string
    showCreateButton?: boolean
    onSearchUpdate: (value: string) => void
    onCreate?: () => void
  }) => {
    searchQuery.value = options.searchQuery
    placeholder.value = options.placeholder || 'Search...'
    showCreateButton.value = options.showCreateButton ?? true
    onSearchUpdate = options.onSearchUpdate
    onCreate = options.onCreate || null
    isActive.value = true
  }

  /**
   * Deregister the search handlers (call in onDeactivated).
   */
  const deregisterMobileSearch = () => {
    isActive.value = false
    onSearchUpdate = null
    onCreate = null
  }

  /**
   * Update the search query (called from MobileSearchBar).
   */
  const handleSearchUpdate = (value: string) => {
    searchQuery.value = value
    if (onSearchUpdate) {
      onSearchUpdate(value)
    }
  }

  /**
   * Trigger create action (called from MobileSearchBar).
   */
  const handleCreate = () => {
    if (onCreate) {
      onCreate()
    }
  }

  /**
   * Update search query from the view (to sync state).
   */
  const updateSearchQuery = (value: string) => {
    searchQuery.value = value
  }

  /**
   * Set create button loading state.
   */
  const setCreateLoading = (loading: boolean) => {
    createButtonLoading.value = loading
  }

  return {
    // Read-only state for MobileSearchBar
    searchQuery: readonly(searchQuery),
    placeholder: readonly(placeholder),
    showCreateButton: readonly(showCreateButton),
    createButtonLoading: readonly(createButtonLoading),
    isActive: readonly(isActive),

    // Methods
    registerMobileSearch,
    deregisterMobileSearch,
    handleSearchUpdate,
    handleCreate,
    updateSearchQuery,
    setCreateLoading
  }
}
