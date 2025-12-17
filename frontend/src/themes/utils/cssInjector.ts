import type { Theme, ThemeColors } from '../types'

/**
 * CSS Variable Injector
 *
 * Applies theme colors as CSS custom properties to the document root.
 * This enables dynamic theme switching without page reload.
 */

/**
 * Converts theme colors to CSS variables and injects them into the DOM
 */
export function applyTheme(theme: Theme, accentOverride?: string): void {
  const colors = { ...theme.colors }

  // Apply accent color override if provided
  if (accentOverride) {
    colors.accent = accentOverride
    colors.accentHover = adjustColor(accentOverride, theme.meta.isDark ? 15 : -15)
    colors.accentMuted = hexToRgba(accentOverride, 0.2)
  }

  const root = document.documentElement

  // Background colors
  root.style.setProperty('--color-app', colors.app)
  root.style.setProperty('--color-surface', colors.surface)
  root.style.setProperty('--color-surface-alt', colors.surfaceAlt)
  root.style.setProperty('--color-surface-hover', colors.surfaceHover)

  // Border colors
  root.style.setProperty('--color-default', colors.default)
  root.style.setProperty('--color-subtle', colors.subtle)
  root.style.setProperty('--color-strong', colors.strong)

  // Text colors
  root.style.setProperty('--color-primary', colors.primary)
  root.style.setProperty('--color-secondary', colors.secondary)
  root.style.setProperty('--color-tertiary', colors.tertiary)

  // Accent colors
  root.style.setProperty('--color-accent', colors.accent)
  root.style.setProperty('--color-accent-hover', colors.accentHover)
  root.style.setProperty('--color-accent-muted', colors.accentMuted)

  // Status colors
  root.style.setProperty('--color-status-success', colors.success)
  root.style.setProperty('--color-status-success-muted', colors.successMuted)
  root.style.setProperty('--color-status-error', colors.error)
  root.style.setProperty('--color-status-error-muted', colors.errorMuted)
  root.style.setProperty('--color-status-warning', colors.warning)
  root.style.setProperty('--color-status-warning-muted', colors.warningMuted)
  root.style.setProperty('--color-status-info', colors.info)
  root.style.setProperty('--color-status-info-muted', colors.infoMuted)

  // Ticket status colors
  root.style.setProperty('--color-status-open', colors.statusOpen)
  root.style.setProperty('--color-status-open-muted', colors.statusOpenMuted)
  root.style.setProperty('--color-status-in-progress', colors.statusInProgress)
  root.style.setProperty('--color-status-in-progress-muted', colors.statusInProgressMuted)
  root.style.setProperty('--color-status-closed', colors.statusClosed)
  root.style.setProperty('--color-status-closed-muted', colors.statusClosedMuted)

  // Priority colors
  root.style.setProperty('--color-priority-high', colors.priorityHigh)
  root.style.setProperty('--color-priority-high-muted', colors.priorityHighMuted)
  root.style.setProperty('--color-priority-medium', colors.priorityMedium)
  root.style.setProperty('--color-priority-medium-muted', colors.priorityMediumMuted)
  root.style.setProperty('--color-priority-low', colors.priorityLow)
  root.style.setProperty('--color-priority-low-muted', colors.priorityLowMuted)

  // Shadows
  root.style.setProperty('--shadow-inset-dark', colors.shadowDark)
  root.style.setProperty('--shadow-inset-light', colors.shadowLight)

  // Syntax colors (if defined)
  if (colors.syntax) {
    Object.entries(colors.syntax).forEach(([key, value]) => {
      root.style.setProperty(`--color-syntax-${key}`, value)
    })
  }

  // Update dark class for Tailwind compatibility
  if (theme.meta.isDark) {
    root.classList.add('dark')
  } else {
    root.classList.remove('dark')
  }

  // Store active theme ID for reference
  root.dataset.theme = theme.meta.id
}

/**
 * Adjusts a hex color by a percentage (positive = lighter, negative = darker)
 */
function adjustColor(hex: string, percent: number): string {
  // Remove # if present
  const cleanHex = hex.replace('#', '')

  // Parse RGB components
  const num = parseInt(cleanHex, 16)
  const r = (num >> 16) & 0xff
  const g = (num >> 8) & 0xff
  const b = num & 0xff

  // Calculate adjustment
  const amt = Math.round(2.55 * percent)

  // Apply adjustment with clamping
  const newR = Math.max(0, Math.min(255, r + amt))
  const newG = Math.max(0, Math.min(255, g + amt))
  const newB = Math.max(0, Math.min(255, b + amt))

  // Convert back to hex
  return `#${((1 << 24) + (newR << 16) + (newG << 8) + newB).toString(16).slice(1)}`
}

/**
 * Converts a hex color to rgba with specified opacity
 */
function hexToRgba(hex: string, alpha: number): string {
  const cleanHex = hex.replace('#', '')
  const num = parseInt(cleanHex, 16)
  const r = (num >> 16) & 0xff
  const g = (num >> 8) & 0xff
  const b = num & 0xff

  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

/**
 * Gets the current theme ID from the document
 */
export function getCurrentThemeId(): string | undefined {
  return document.documentElement.dataset.theme
}

/**
 * Checks if the current theme is dark
 */
export function isDarkTheme(): boolean {
  return document.documentElement.classList.contains('dark')
}

/**
 * Gets a CSS variable value from the document
 */
export function getCssVariable(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
}
