import { ref, computed, onMounted, onBeforeUnmount, watch } from 'vue'

// Storage keys
const STORAGE_KEYS = {
  collapsed: 'navbarCollapsed',
  docsCollapsed: 'docsCollapsed',
  ticketsCollapsed: 'ticketsCollapsed'
} as const

// Breakpoints matching Tailwind
const BREAKPOINTS = {
  sm: 640,   // Mobile boundary
  lg: 1024   // Desktop boundary
} as const

// Singleton state - shared across all instances
const isCollapsed = ref(false)
const isMobile = ref(false)
const isTablet = ref(false)
const isDesktop = ref(false)
const isCompactNav = ref(false)
const isDocsCollapsed = ref(false)
const isTicketsCollapsed = ref(false)

let initialized = false
let resizeHandler: (() => void) | null = null

/**
 * Composable for managing navbar collapsed/expanded state.
 * Handles responsive behavior, localStorage persistence, and user preferences.
 */
export function useNavbarState() {
  // Load preference from localStorage
  const loadPreference = (key: string, defaultValue: boolean): boolean => {
    const stored = localStorage.getItem(key)
    return stored !== null ? stored === 'true' : defaultValue
  }

  // Save preference to localStorage
  const savePreference = (key: string, value: boolean) => {
    localStorage.setItem(key, value.toString())
  }

  // Update screen size flags
  const updateScreenSize = () => {
    const width = window.innerWidth
    const height = window.innerHeight

    const wasMobile = isMobile.value

    isMobile.value = width < BREAKPOINTS.sm
    isTablet.value = width >= BREAKPOINTS.sm && width < BREAKPOINTS.lg
    isDesktop.value = width >= BREAKPOINTS.lg
    isCompactNav.value = height < 750

    // Only auto-change collapsed state when transitioning to/from mobile
    if (isMobile.value && !wasMobile) {
      // Entering mobile: always collapse (bottom nav takes over)
      isCollapsed.value = true
    } else if (!isMobile.value && wasMobile) {
      // Leaving mobile: restore user preference
      isCollapsed.value = loadPreference(STORAGE_KEYS.collapsed, false)
    }
  }

  // Toggle collapsed state (user action)
  const toggleCollapsed = () => {
    if (isMobile.value) return // Don't toggle on mobile

    isCollapsed.value = !isCollapsed.value
    savePreference(STORAGE_KEYS.collapsed, isCollapsed.value)
  }

  // Toggle documentation section
  const toggleDocs = () => {
    isDocsCollapsed.value = !isDocsCollapsed.value
    savePreference(STORAGE_KEYS.docsCollapsed, isDocsCollapsed.value)
  }

  // Toggle tickets section
  const toggleTickets = () => {
    isTicketsCollapsed.value = !isTicketsCollapsed.value
    savePreference(STORAGE_KEYS.ticketsCollapsed, isTicketsCollapsed.value)
  }

  // Set collapsed state directly (for programmatic control)
  const setCollapsed = (value: boolean) => {
    if (isMobile.value) return
    isCollapsed.value = value
    savePreference(STORAGE_KEYS.collapsed, value)
  }

  // Initialize on first use
  const initialize = () => {
    if (initialized) return

    // Load stored preferences
    isDocsCollapsed.value = loadPreference(STORAGE_KEYS.docsCollapsed, false)
    isTicketsCollapsed.value = loadPreference(STORAGE_KEYS.ticketsCollapsed, false)

    // Set initial screen size
    const width = window.innerWidth
    const height = window.innerHeight
    isMobile.value = width < BREAKPOINTS.sm
    isTablet.value = width >= BREAKPOINTS.sm && width < BREAKPOINTS.lg
    isDesktop.value = width >= BREAKPOINTS.lg
    isCompactNav.value = height < 750

    // Set initial collapsed state
    if (isMobile.value) {
      isCollapsed.value = true
    } else {
      isCollapsed.value = loadPreference(STORAGE_KEYS.collapsed, false)
    }

    // Add resize listener
    resizeHandler = updateScreenSize
    window.addEventListener('resize', resizeHandler)

    initialized = true
  }

  // Cleanup
  const cleanup = () => {
    if (resizeHandler) {
      window.removeEventListener('resize', resizeHandler)
      resizeHandler = null
    }
    initialized = false
  }

  // Computed: should show sidebar (not on mobile)
  const showSidebar = computed(() => !isMobile.value)

  // Computed: should show mobile nav
  const showMobileNav = computed(() => isMobile.value)

  // Computed: sidebar width class
  const sidebarWidthClass = computed(() => isCollapsed.value ? 'w-16' : 'w-64')

  return {
    // State (readonly for consumers)
    isCollapsed: computed(() => isCollapsed.value),
    isMobile: computed(() => isMobile.value),
    isTablet: computed(() => isTablet.value),
    isDesktop: computed(() => isDesktop.value),
    isCompactNav: computed(() => isCompactNav.value),
    isDocsCollapsed: computed(() => isDocsCollapsed.value),
    isTicketsCollapsed: computed(() => isTicketsCollapsed.value),

    // Computed helpers
    showSidebar,
    showMobileNav,
    sidebarWidthClass,

    // Actions
    toggleCollapsed,
    toggleDocs,
    toggleTickets,
    setCollapsed,

    // Lifecycle
    initialize,
    cleanup
  }
}
