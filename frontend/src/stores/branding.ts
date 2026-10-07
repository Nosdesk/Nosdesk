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
import { logger } from '@nosdesk/core/utils/logger'
import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import brandingService, { type BrandingConfig } from '@nosdesk/core/services/brandingService'
import { activeWorkspaceSlugRef } from '@/services/activeWorkspace'

const BRANDING_CACHE_PREFIX = 'nosdesk_branding_cache'

/**
 * Cache key for the workspace currently on screen.
 *
 * Scoped per workspace because the cache exists to paint branding before the
 * fetch resolves: with one shared key, switching workspaces paints the previous
 * tenant's logo and colours until the new config arrives. Falls back to the
 * bare key when no slug is set, which is host mode and self-hosted, where there
 * is only ever one workspace.
 */
function brandingCacheKey(): string {
  const slug = activeWorkspaceSlugRef.value
  return slug ? `${BRANDING_CACHE_PREFIX}:${slug}` : BRANDING_CACHE_PREFIX
}

const DEFAULT_BRANDING: BrandingConfig = {
  app_name: 'Nosdesk',
  logo_url: null,
  logo_light_url: null,
  favicon_url: null,
  primary_color: null,
  updated_at: null,
}

/**
 * Only the fields a page displays. Checked at run time, not just by type: a
 * caller can hand over the admin settings (structurally a `BrandingConfig`
 * too), and a cache written by an older version can hold more.
 */
function toPublicBranding(source: Partial<BrandingConfig>): BrandingConfig {
  return {
    app_name: source.app_name ?? DEFAULT_BRANDING.app_name,
    logo_url: source.logo_url ?? null,
    logo_light_url: source.logo_light_url ?? null,
    favicon_url: source.favicon_url ?? null,
    primary_color: source.primary_color ?? null,
    updated_at: source.updated_at ?? null,
  }
}

/**
 * Load cached branding from localStorage
 */
function loadCachedBranding(): BrandingConfig | null {
  try {
    const cached = localStorage.getItem(brandingCacheKey())
    if (cached) {
      const parsed: unknown = JSON.parse(cached)
      if (parsed && typeof parsed === 'object') {
        const branding = toPublicBranding(parsed as Partial<BrandingConfig>)
        // Rewrite a cache that held more than this.
        if (Object.keys(parsed).length !== Object.keys(branding).length) {
          saveBrandingCache(branding)
        }
        return branding
      }
    }
  } catch {
    // Ignore parse errors
  }
  return null
}

/**
 * Save branding to localStorage cache
 */
function saveBrandingCache(config: BrandingConfig): void {
  try {
    localStorage.setItem(brandingCacheKey(), JSON.stringify(config))
  } catch {
    // Ignore storage errors
  }
}

export const useBrandingStore = defineStore('branding', () => {
  // Load cached branding immediately to prevent flash
  const cachedBranding = loadCachedBranding()

  // Branding configuration - use cached values if available
  const config = ref<BrandingConfig>(cachedBranding || { ...DEFAULT_BRANDING })

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

  /** Show `brandingConfig`, cache it for the next visit and re-apply the theme. */
  function applyLoaded(loaded: BrandingConfig): void {
    const brandingConfig = toPublicBranding(loaded)
    config.value = brandingConfig
    isLoaded.value = true
    saveBrandingCache(brandingConfig)
    applyBrandingToDocument()
    reapplyTheme()
    logger.debug('Branding loaded:', brandingConfig)
  }

  /** Re-apply the theme so it picks up the branding colour. Imported lazily to
   *  avoid a circular dependency. */
  function reapplyTheme(): void {
    import('@/stores/theme').then(({ useThemeStore }) => {
      const themeStore = useThemeStore()
      themeStore.setTheme(themeStore.currentTheme)
    })
  }

  /**
   * Load branding from the public route, which resolves the workspace from the
   * Host: self-hosted, host mode, the portal and sign-in pages. While a
   * workspace is selected on the single-origin agent app it does nothing, since
   * the Host names no workspace there; `loadWorkspaceBranding` owns it, and a
   * result that lands after a workspace was selected is dropped.
   */
  async function loadBranding(): Promise<void> {
    if (isLoading.value || activeWorkspaceSlugRef.value) return

    try {
      isLoading.value = true
      const brandingConfig = await brandingService.getBrandingConfig()
      if (activeWorkspaceSlugRef.value) return
      applyLoaded(brandingConfig)
    } catch (error) {
      logger.error('Failed to load branding:', error)
      // Keep defaults/cached values on error
    } finally {
      isLoading.value = false
    }
  }

  /**
   * Load the selected workspace's branding through the authenticated route.
   * Only for a signed-in member with a workspace selected (the workspace guard
   * calls it on entering one): a 401 there would end the session. Paints the
   * workspace's cached branding first, so a reload or a switch doesn't flash
   * the defaults, and drops a result for a workspace no longer selected.
   */
  async function loadWorkspaceBranding(): Promise<void> {
    const slug = activeWorkspaceSlugRef.value
    if (!slug) return loadBranding()

    const cached = loadCachedBranding()
    if (cached) {
      config.value = cached
      reapplyTheme()
    }
    try {
      const brandingConfig = await brandingService.getWorkspaceBranding()
      if (activeWorkspaceSlugRef.value !== slug) return
      applyLoaded(brandingConfig)
    } catch (error) {
      logger.error('Failed to load workspace branding:', error)
    }
  }

  /**
   * Update the branding configuration. Keeps only the public fields, so the
   * admin settings view can pass its full settings.
   */
  function updateConfig(newConfig: BrandingConfig): void {
    const branding = toPublicBranding(newConfig)
    config.value = branding
    saveBrandingCache(branding)
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
    config.value = { ...DEFAULT_BRANDING }
    // Clear the cache
    try {
      localStorage.removeItem(brandingCacheKey())
    } catch {
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
    loadWorkspaceBranding,
    updateConfig,
    getPageTitle,
    resetBranding,
  }
})
