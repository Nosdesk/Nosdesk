import { ref, computed } from 'vue'
import { BREAKPOINTS } from './useMobileDetection'

// Storage keys
const STORAGE_KEYS = {
  collapsed: 'navbarCollapsed',
  docsCollapsed: 'docsCollapsed',
  ticketsCollapsed: 'ticketsCollapsed'
} as const

// --- Synchronous initial state ----------------------------------------
//
// The singleton refs are seeded eagerly at module load — reading
// localStorage + viewport size before first paint — so the sidebar
// renders at its correct width on the very first frame. Setting these
// in onMounted instead caused an expand->collapse flash (default
// `false`, corrected after mount). This is a browser-only SPA, so
// `window` / `localStorage` are always available here; the typeof
// guard is cheap insurance.
const hasWindow = typeof window !== 'undefined'

const storedBool = (key: string, fallback: boolean): boolean => {
  if (!hasWindow) return fallback
  const stored = localStorage.getItem(key)
  return stored !== null ? stored === 'true' : fallback
}

const initialViewportWidth = hasWindow ? window.innerWidth : BREAKPOINTS.lg
const initialViewportHeight = hasWindow ? window.innerHeight : 800
const initialIsMobile = initialViewportWidth < BREAKPOINTS.sm

// Singleton state - shared across all instances
const isCollapsed = ref(initialIsMobile ? true : storedBool(STORAGE_KEYS.collapsed, false))
const isMobile = ref(initialIsMobile)
const isTablet = ref(
  initialViewportWidth >= BREAKPOINTS.sm && initialViewportWidth < BREAKPOINTS.lg,
)
const isDesktop = ref(initialViewportWidth >= BREAKPOINTS.lg)
const isCompactNav = ref(initialViewportHeight < 750)
const isDocsCollapsed = ref(storedBool(STORAGE_KEYS.docsCollapsed, false))
const isTicketsCollapsed = ref(storedBool(STORAGE_KEYS.ticketsCollapsed, false))

let initialized = false
let resizeTimeout: ReturnType<typeof setTimeout> | null = null

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

  // Initialize on first use. State (collapsed flag, viewport flags) is
  // already seeded synchronously at module load, so this only attaches
  // the live resize listener — no re-seeding, no post-mount flash.
  const initialize = () => {
    if (initialized) return
    // Add debounced resize listener (150ms matches useMobileDetection)
    window.addEventListener('resize', debouncedUpdateScreenSize)
    initialized = true
  }

  const debouncedUpdateScreenSize = () => {
    if (resizeTimeout) clearTimeout(resizeTimeout)
    resizeTimeout = setTimeout(updateScreenSize, 150)
  }

  // Cleanup
  const cleanup = () => {
    window.removeEventListener('resize', debouncedUpdateScreenSize)
    if (resizeTimeout) {
      clearTimeout(resizeTimeout)
      resizeTimeout = null
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
