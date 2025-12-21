/**
 * Theme Store
 *
 * Manages the application theme state, including:
 * - Theme selection (system, light, dark, or custom theme IDs)
 * - Accent color overrides
 * - Theme application to DOM
 * - Backend synchronization
 */
import { logger } from '@/utils/logger'
import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import userService from '@/services/userService'
import type { User } from '@/services/userService'
import {
  getTheme,
  getAllThemes,
  getLightThemes,
  getDarkThemes,
  hasTheme,
  applyTheme,
} from '@/themes'
import type { Theme, ThemeMode } from '@/themes'

export const useThemeStore = defineStore('theme', () => {
  // Current theme selection (theme ID or 'system')
  const savedTheme = localStorage.getItem('theme') || 'system'
  const currentTheme = ref<ThemeMode>(savedTheme)

  // Optional accent color override
  const savedAccent = localStorage.getItem('accentColor')
  const accentColorOverride = ref<string | null>(savedAccent || null)

  // Color blind friendly mode - uses shapes instead of colors for status indicators
  const savedColorBlindMode = localStorage.getItem('colorBlindMode') === 'true'
  const colorBlindMode = ref<boolean>(savedColorBlindMode)

  // Syncing state
  const isSyncing = ref<boolean>(false)

  // System preference tracking
  const systemPrefersDark = ref(
    window.matchMedia('(prefers-color-scheme: dark)').matches
  )

  // Listen for system preference changes
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (e) => {
    systemPrefersDark.value = e.matches
    if (currentTheme.value === 'system') {
      applyCurrentTheme()
    }
  })

  /**
   * Get the resolved theme object
   * Resolves 'system' to light or dark based on OS preference
   */
  const effectiveTheme = computed<Theme>(() => {
    if (currentTheme.value === 'system') {
      return systemPrefersDark.value
        ? getTheme('dark')!
        : getTheme('light')!
    }

    // Return the selected theme, fallback to light if not found
    return getTheme(currentTheme.value) ?? getTheme('light')!
  })

  /**
   * Whether the current effective theme is dark
   */
  const isDarkMode = computed(() => effectiveTheme.value.meta.isDark)

  /**
   * Available themes for UI display
   */
  const availableThemes = computed(() => getAllThemes())
  const lightThemes = computed(() => getLightThemes())
  const darkThemes = computed(() => getDarkThemes())

  /**
   * Apply the current theme to the DOM
   */
  function applyCurrentTheme(): void {
    applyTheme(effectiveTheme.value, accentColorOverride.value ?? undefined)
  }

  /**
   * Set the current theme
   */
  function setTheme(themeId: ThemeMode): void {
    // Validate theme exists (or is 'system')
    if (themeId !== 'system' && !hasTheme(themeId)) {
      logger.warn('Invalid theme ID:', themeId)
      return
    }

    currentTheme.value = themeId
    localStorage.setItem('theme', themeId)
    applyCurrentTheme()

    // E-Paper theme should automatically enable color blind mode
    // since it's monochromatic and relies on shapes for distinction
    if (themeId === 'epaper') {
      setColorBlindMode(true)
    }
  }

  /**
   * Set the accent color override
   */
  function setAccentColor(color: string | null): void {
    accentColorOverride.value = color
    if (color) {
      localStorage.setItem('accentColor', color)
    } else {
      localStorage.removeItem('accentColor')
    }
    applyCurrentTheme()
  }

  /**
   * Set color blind friendly mode
   * When enabled, status indicators use shapes instead of relying solely on colors
   */
  function setColorBlindMode(enabled: boolean): void {
    colorBlindMode.value = enabled
    if (enabled) {
      localStorage.setItem('colorBlindMode', 'true')
    } else {
      localStorage.removeItem('colorBlindMode')
    }
  }

  /**
   * Toggle between light and dark modes
   * If on a specific theme, switches to the opposite base theme
   */
  function toggleTheme(): void {
    if (isDarkMode.value) {
      setTheme('light')
    } else {
      setTheme('dark')
    }
  }

  /**
   * Sync theme to backend user profile
   */
  async function syncThemeToBackend(userUuid: string): Promise<boolean> {
    if (!userUuid) {
      logger.warn('Cannot sync theme: no user UUID provided')
      return false
    }

    try {
      isSyncing.value = true

      await userService.updateUser(userUuid, {
        theme: currentTheme.value
      })

      logger.debug('Theme synced to backend:', currentTheme.value)
      return true
    } catch (error) {
      logger.error('Failed to sync theme to backend:', error)
      return false
    } finally {
      isSyncing.value = false
    }
  }

  /**
   * Load theme from user profile
   */
  function loadThemeFromUser(user: User | null): void {
    if (user && user.theme) {
      logger.debug('Loading theme from user profile:', user.theme)
      setTheme(user.theme as ThemeMode)
    }
  }

  // Initialize theme on store creation
  applyCurrentTheme()

  // Watch for effective theme changes
  watch(effectiveTheme, () => {
    applyCurrentTheme()
  })

  return {
    // State
    currentTheme,
    effectiveTheme,
    isDarkMode,
    accentColorOverride,
    colorBlindMode,
    isSyncing,

    // Theme lists for UI
    availableThemes,
    lightThemes,
    darkThemes,

    // Actions
    setTheme,
    setAccentColor,
    setColorBlindMode,
    toggleTheme,
    syncThemeToBackend,
    loadThemeFromUser,
  }
})
