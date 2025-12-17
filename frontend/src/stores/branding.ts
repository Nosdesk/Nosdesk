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

export const useBrandingStore = defineStore('branding', () => {
  // Branding configuration
  const config = ref<BrandingConfig>({
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

      // Apply branding to the document
      applyBrandingToDocument()

      logger.debug('Branding loaded:', brandingConfig)
    } catch (error) {
      logger.error('Failed to load branding:', error)
      // Keep defaults on error
    } finally {
      isLoading.value = false
    }
  }

  /**
   * Update the branding configuration
   */
  function updateConfig(newConfig: BrandingConfig): void {
    config.value = newConfig
    applyBrandingToDocument()
  }

  /**
   * Apply branding to the document (favicon and default title)
   */
  function applyBrandingToDocument(): void {
    // Update favicon if custom one is set
    if (config.value.favicon_url) {
      brandingService.updateFavicon(config.value.favicon_url)
    }

    // Store the app name for use in page titles
    // The actual title updates are handled by useTitleManager composable
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
    brandingService.resetFavicon()
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
