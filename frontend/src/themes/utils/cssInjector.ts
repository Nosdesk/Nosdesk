import type { Theme } from '../types'

/**
 * CSS Variable Injector
 *
 * Applies theme colors as CSS custom properties via a <style> element.
 * This enables dynamic theme switching without page reload.
 */

const STYLE_ID = 'theme-variables'

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

  // Build CSS variables
  const cssVars = `
:root {
  /* Background colors */
  --color-app: ${colors.app};
  --color-surface: ${colors.surface};
  --color-surface-alt: ${colors.surfaceAlt};
  --color-surface-hover: ${colors.surfaceHover};

  /* Border colors */
  --color-default: ${colors.default};
  --color-subtle: ${colors.subtle};
  --color-strong: ${colors.strong};

  /* Text colors */
  --color-primary: ${colors.primary};
  --color-secondary: ${colors.secondary};
  --color-tertiary: ${colors.tertiary};

  /* Accent colors */
  --color-accent: ${colors.accent};
  --color-accent-hover: ${colors.accentHover};
  --color-accent-muted: ${colors.accentMuted};

  /* Status colors */
  --color-status-success: ${colors.success};
  --color-status-success-muted: ${colors.successMuted};
  --color-status-error: ${colors.error};
  --color-status-error-muted: ${colors.errorMuted};
  --color-status-warning: ${colors.warning};
  --color-status-warning-muted: ${colors.warningMuted};
  --color-status-info: ${colors.info};
  --color-status-info-muted: ${colors.infoMuted};

  /* Ticket status colors */
  --color-status-open: ${colors.statusOpen};
  --color-status-open-muted: ${colors.statusOpenMuted};
  --color-status-in-progress: ${colors.statusInProgress};
  --color-status-in-progress-muted: ${colors.statusInProgressMuted};
  --color-status-closed: ${colors.statusClosed};
  --color-status-closed-muted: ${colors.statusClosedMuted};

  /* Priority colors */
  --color-priority-high: ${colors.priorityHigh};
  --color-priority-high-muted: ${colors.priorityHighMuted};
  --color-priority-medium: ${colors.priorityMedium};
  --color-priority-medium-muted: ${colors.priorityMediumMuted};
  --color-priority-low: ${colors.priorityLow};
  --color-priority-low-muted: ${colors.priorityLowMuted};

  /* Shadows */
  --shadow-inset-dark: ${colors.shadowDark};
  --shadow-inset-light: ${colors.shadowLight};
${colors.syntax ? Object.entries(colors.syntax).map(([key, value]) => `
  /* Syntax: ${key} */
  --color-syntax-${key}: ${value};`).join('') : ''}
}
`.trim()

  // Get or create style element
  let styleEl = document.getElementById(STYLE_ID) as HTMLStyleElement | null
  if (!styleEl) {
    styleEl = document.createElement('style')
    styleEl.id = STYLE_ID
    document.head.appendChild(styleEl)
  }

  // Update styles
  styleEl.textContent = cssVars

  // Update dark class for Tailwind compatibility
  const root = document.documentElement
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
  const cleanHex = hex.replace('#', '')
  const num = parseInt(cleanHex, 16)
  const r = (num >> 16) & 0xff
  const g = (num >> 8) & 0xff
  const b = num & 0xff

  const amt = Math.round(2.55 * percent)

  const newR = Math.max(0, Math.min(255, r + amt))
  const newG = Math.max(0, Math.min(255, g + amt))
  const newB = Math.max(0, Math.min(255, b + amt))

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
