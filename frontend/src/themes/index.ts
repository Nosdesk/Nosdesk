/**
 * Theme Registry
 *
 * Central registry for all available themes.
 * Exports utilities for theme discovery and access.
 */

import type { Theme, ThemeMeta } from './types'

// Base themes
import { lightTheme } from './base/light'
import { darkTheme } from './base/dark'

// Preset themes
import { nordTheme } from './presets/nord'
import { tokyoNightTheme } from './presets/tokyo-night'
import { draculaTheme } from './presets/dracula'
import { gruvboxLightTheme, gruvboxDarkTheme } from './presets/gruvbox'
import { everforestLightTheme, everforestDarkTheme } from './presets/everforest'
import { catppuccinLatteTheme, catppuccinMochaTheme } from './presets/catppuccin'
import { oneDarkTheme } from './presets/one-dark'
import { ePaperTheme } from './presets/epaper'
import { pureBlackTheme } from './presets/pure-black'
import { solarizedLightTheme, solarizedDarkTheme } from './presets/solarized'
import { nossalDarkTheme } from './presets/nossal'
import { redHorizonTheme } from './presets/red-horizon'
import { christmasTheme } from './presets/christmas'

// Re-export types
export * from './types'

// Re-export utilities
export { applyTheme, getCurrentThemeId, isDarkTheme, getCssVariable } from './utils/cssInjector'

/**
 * Seasonal theme configuration
 * Maps theme IDs to the months (1-12) when they should be available
 */
const SEASONAL_THEMES: Map<string, number[]> = new Map([
  ['christmas', [12]], // December only
])

/**
 * Check if a seasonal theme is currently available
 */
export function isSeasonalThemeAvailable(themeId: string): boolean {
  const months = SEASONAL_THEMES.get(themeId)
  if (!months) return true // Not a seasonal theme, always available

  const currentMonth = new Date().getMonth() + 1 // getMonth() is 0-indexed
  return months.includes(currentMonth)
}

/**
 * Check if a theme is seasonal (regardless of current availability)
 */
export function isSeasonalTheme(themeId: string): boolean {
  return SEASONAL_THEMES.has(themeId)
}

/**
 * Registry of all available themes
 */
export const themeRegistry: Map<string, Theme> = new Map([
  // Base themes
  ['light', lightTheme],
  ['dark', darkTheme],
  // Preset themes
  ['nord', nordTheme],
  ['tokyo-night', tokyoNightTheme],
  ['dracula', draculaTheme],
  ['gruvbox-light', gruvboxLightTheme],
  ['gruvbox-dark', gruvboxDarkTheme],
  ['everforest-light', everforestLightTheme],
  ['everforest-dark', everforestDarkTheme],
  ['catppuccin-latte', catppuccinLatteTheme],
  ['catppuccin-mocha', catppuccinMochaTheme],
  ['one-dark', oneDarkTheme],
  ['solarized-light', solarizedLightTheme],
  ['solarized-dark', solarizedDarkTheme],
  ['nossal-dark', nossalDarkTheme],
  ['epaper', ePaperTheme],
  ['pure-black', pureBlackTheme],
  ['red-horizon', redHorizonTheme],
  // Seasonal themes
  ['christmas', christmasTheme],
])

/**
 * Get a theme by ID
 */
export function getTheme(id: string): Theme | undefined {
  return themeRegistry.get(id)
}

/**
 * Get all available themes (respects seasonal availability)
 */
export function getAllThemes(): Theme[] {
  return Array.from(themeRegistry.entries())
    .filter(([id]) => isSeasonalThemeAvailable(id))
    .map(([, theme]) => theme)
}

/**
 * Get all theme metadata (for display in pickers)
 */
export function getAllThemeMeta(): ThemeMeta[] {
  return getAllThemes().map((t) => t.meta)
}

/**
 * Get themes filtered by category
 */
export function getThemesByCategory(category: 'builtin' | 'community' | 'custom'): Theme[] {
  return getAllThemes().filter((t) => t.meta.category === category)
}

/**
 * Get all light themes
 */
export function getLightThemes(): Theme[] {
  return getAllThemes().filter((t) => !t.meta.isDark)
}

/**
 * Get all dark themes
 */
export function getDarkThemes(): Theme[] {
  return getAllThemes().filter((t) => t.meta.isDark)
}

/**
 * Check if a theme ID exists
 */
export function hasTheme(id: string): boolean {
  return themeRegistry.has(id)
}

/**
 * Register a custom theme
 * Returns false if a theme with that ID already exists
 */
export function registerTheme(theme: Theme): boolean {
  if (themeRegistry.has(theme.meta.id)) {
    console.warn(`Theme "${theme.meta.id}" already exists, skipping registration`)
    return false
  }

  themeRegistry.set(theme.meta.id, theme)
  return true
}

/**
 * Unregister a custom theme
 * Only custom themes can be unregistered
 */
export function unregisterTheme(id: string): boolean {
  const theme = themeRegistry.get(id)

  if (!theme) {
    return false
  }

  if (theme.meta.category !== 'custom') {
    console.warn(`Cannot unregister non-custom theme "${id}"`)
    return false
  }

  themeRegistry.delete(id)
  return true
}
