/**
 * Branding Store
 *
 * Manages the application branding state, including:
 * - App name
 * - Logo URLs (dark and light variants)
 * - Favicon URL
 * - Primary color
 * - Applying branding to the DOM (favicon, title)
 */
import { logger } from '@/utils/logger'
import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import brandingService, { type BrandingConfig } from '@/services/brandingService'

const BRANDING_CACHE_KEY = 'nosdesk_branding_cache'

/**
 * Load cached branding from localStorage
 */
function loadCachedBranding(): BrandingConfig | null {
  try {
    const cached = localStorage.getItem(BRANDING_CACHE_KEY)
    if (cached) {
      return JSON.parse(cached)
    }
  } catch (e) {
    // Ignore parse errors
  }
  return null
}

/**
 * Save branding to localStorage cache
 */
function saveBrandingCache(config: BrandingConfig): void {
  try {
    localStorage.setItem(BRANDING_CACHE_KEY, JSON.stringify(config))
  } catch (e) {
    // Ignore storage errors
  }
}

export const useBrandingStore = defineStore('branding', () => {
  // Load cached branding immediately to prevent flash
  const cachedBranding = loadCachedBranding()

  // Branding configuration - use cached values if available
  const config = ref<BrandingConfig>(cachedBranding || {
    app_name: 'Nosdesk',
    logo_url: null,
    logo_light_url: null,
    favicon_url: null,
    primary_color: null,
    updated_at: null
  })

  // Loading state
  const isLoading = ref(false)
  const isLoaded = ref(false)

  /**
   * Get the app name
   */
  const appName = computed(() => config.value.app_name || 'Nosdesk')

  /**
   * Get the logo URL for the current theme
   * @param isDark - Whether to get the dark theme logo
   */
  const getLogoUrl = (isDark: boolean = false) => {
    if (isDark) {
      // For dark mode, prefer the main logo (designed for dark backgrounds)
      return config.value.logo_url
    } else {
      // For light mode, prefer the light logo variant if available
      return config.value.logo_light_url || config.value.logo_url
    }
  }

  /**
   * Get the favicon URL
   */
  const faviconUrl = computed(() => config.value.favicon_url)

  /**
   * Get the primary brand color
   */
  const primaryColor = computed(() => config.value.primary_color)

  /**
   * Check if custom logo is configured
   */
  const hasCustomLogo = computed(() => !!config.value.logo_url)

  /**
   * Check if custom favicon is configured
   */
  const hasCustomFavicon = computed(() => !!config.value.favicon_url)

  /**
   * Load branding configuration from the backend
   */
  async function loadBranding(): Promise<void> {
    if (isLoading.value) return

    try {
      isLoading.value = true
      const brandingConfig = await brandingService.getBrandingConfig()
      config.value = brandingConfig
      isLoaded.value = true

      // Cache branding to localStorage for instant load on next visit
      saveBrandingCache(brandingConfig)

      // Apply branding to the document
      applyBrandingToDocument()

      // Re-apply theme to pick up branding color changes
      // Import dynamically to avoid circular dependency
      import('@/stores/theme').then(({ useThemeStore }) => {
        const themeStore = useThemeStore()
        // Trigger theme reapplication by calling setTheme with current theme
        const currentTheme = themeStore.currentTheme
        themeStore.setTheme(currentTheme)
      })

      logger.debug('Branding loaded:', brandingConfig)
    } catch (error) {
      logger.error('Failed to load branding:', error)
      // Keep defaults/cached values on error
    } finally {
      isLoading.value = false
    }
  }

  /**
   * Update the branding configuration
   */
  function updateConfig(newConfig: BrandingConfig): void {
    config.value = newConfig
    saveBrandingCache(newConfig)
    applyBrandingToDocument()
  }

  /**
   * Apply branding to the document
   * Note: Favicon is now handled reactively by useFavicon composable in App.vue
   */
  function applyBrandingToDocument(): void {
    // Favicon is handled by useFavicon composable watching faviconUrl
    // App name for page titles is handled by useTitleManager composable
  }

  /**
   * Get the full page title with app name
   */
  function getPageTitle(pageTitle?: string): string {
    if (!pageTitle) {
      return appName.value
    }
    return `${pageTitle} | ${appName.value}`
  }

  /**
   * Reset branding to defaults
   */
  function resetBranding(): void {
    config.value = {
      app_name: 'Nosdesk',
      logo_url: null,
      logo_light_url: null,
      favicon_url: null,
      primary_color: null,
      updated_at: null
    }
    // Clear the cache
    try {
      localStorage.removeItem(BRANDING_CACHE_KEY)
    } catch (e) {
      // Ignore storage errors
    }
    // Favicon reset is handled automatically by useFavicon composable
    // when faviconUrl becomes null
  }

  return {
    // State
    config,
    isLoading,
    isLoaded,

    // Computed
    appName,
    faviconUrl,
    primaryColor,
    hasCustomLogo,
    hasCustomFavicon,

    // Actions
    getLogoUrl,
    loadBranding,
    updateConfig,
    getPageTitle,
    resetBranding,
  }
})
