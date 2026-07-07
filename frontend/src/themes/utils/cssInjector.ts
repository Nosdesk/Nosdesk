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
    // Workspace-branding overrides reset the on-accent text colour
    // — the theme-provided one was paired with the theme's accent,
    // not the override.
    colors.accentForeground = undefined
  }

  // On-accent foreground: text colour rendered on `bg-accent`
  // (button labels, badges). If the theme didn't pin one, pick
  // black or white via WCAG luminance against the actual accent
  // fill. This makes the choice Just Work for any accent value,
  // including workspace-branding overrides — and keeps themes
  // whose accent is dark (e.g. epaper's `#000000`) from rendering
  // invisible black-on-black labels, which was the symptom that
  // surfaced this whole token.
  const onAccent = colors.accentForeground ?? pickAccentForeground(colors.accent)

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
  --color-on-accent: ${onAccent};

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

  // Cache the resolved app background + accent for the next cold
  // launch. The pre-mount splash (`public/splash.js`) reads this so
  // its background and the "N" match the user's real theme before
  // any Vue/JS has run. Mirrors the `nosdesk_branding_cache` pattern
  // read by `branding-init.js`. Best-effort: localStorage can throw
  // in private mode, and a stale cache only costs one off-theme
  // launch frame.
  try {
    localStorage.setItem(
      'nosdesk_launch_theme',
      JSON.stringify({ app: colors.app, accent: colors.accent }),
    )
  } catch {
    // ignore: splash falls back to prefers-color-scheme + dark brand
  }
}

/**
 * Pick `#000000` or `#ffffff` for text rendered on top of `accent`,
 * choosing whichever scores higher WCAG contrast against the
 * accent fill. Uses the standard WCAG 2.x relative-luminance
 * formula (sRGB → linear via the per-channel piecewise transform,
 * then weighted sum), then compares the resulting (L+0.05)/0.05
 * (black contrast) to 1.05/(L+0.05) (white contrast).
 *
 * The brand orange `#FF6B1A` scores 7.4:1 with black vs 2.85:1
 * with white — picks black. Epaper's `#000000` accent scores
 * 21:1 with white vs undefined-but-zero with black — picks
 * white. Slate's cyan `#06B6D4` scores 9.2:1 black vs 2.3:1
 * white — black. So on. Every theme's accent gets an AA-passing
 * foreground without per-theme bookkeeping.
 */
function pickAccentForeground(accent: string): string {
  const hex = accent.replace('#', '')
  if (hex.length !== 6) return '#000000' // bail to default on malformed input

  const toLinear = (c: number) => {
    const cs = c / 255
    return cs <= 0.03928 ? cs / 12.92 : Math.pow((cs + 0.055) / 1.055, 2.4)
  }
  const r = toLinear(parseInt(hex.slice(0, 2), 16))
  const g = toLinear(parseInt(hex.slice(2, 4), 16))
  const b = toLinear(parseInt(hex.slice(4, 6), 16))
  const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b

  const whiteContrast = 1.05 / (luminance + 0.05)
  const blackContrast = (luminance + 0.05) / 0.05
  return whiteContrast > blackContrast ? '#ffffff' : '#000000'
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
