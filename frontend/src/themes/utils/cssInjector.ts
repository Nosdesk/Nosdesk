import type { Theme, ThemeColors } from '../types'
import { lightTheme } from '../base/light'
import { darkTheme } from '../base/dark'
import { contrast, parseColour, readableOn, type Rgb } from './readable'
import { createNoncedStyleElement } from '@/utils/cspNonce'

/**
 * CSS Variable Injector
 *
 * Applies theme colors as CSS custom properties via a <style> element.
 * This enables dynamic theme switching without page reload.
 */

const STYLE_ID = 'theme-variables'

/** The overdue text colour a theme renders with: its own, else the base
 *  light or dark one. */
export function overdueColor(theme: Theme): string {
  const base = theme.meta.isDark ? darkTheme : lightTheme
  return theme.colors.overdue ?? base.colors.overdue ?? ''
}

/**
 * The colours that carry meaning as text, each with the tint it sits on
 * in chips. `text-status-error`, `text-accent` and the rest read the
 * `--color-<name>-text` version of each (see `--text-color-*` in
 * main.css), derived here per theme.
 */
export const TEXT_COLOURS = [
  ['status-success', 'success', 'successMuted'],
  ['status-error', 'error', 'errorMuted'],
  ['status-warning', 'warning', 'warningMuted'],
  ['status-info', 'info', 'infoMuted'],
  ['status-open', 'statusOpen', 'statusOpenMuted'],
  ['status-in-progress', 'statusInProgress', 'statusInProgressMuted'],
  ['status-closed', 'statusClosed', 'statusClosedMuted'],
  ['priority-high', 'priorityHigh', 'priorityHighMuted'],
  ['priority-medium', 'priorityMedium', 'priorityMediumMuted'],
  ['priority-low', 'priorityLow', 'priorityLowMuted'],
  ['accent', 'accent', 'accentMuted'],
  ['accent-hover', 'accentHover', 'accentMuted'],
] as const satisfies ReadonlyArray<readonly [string, keyof ThemeColors, keyof ThemeColors]>

/** The backgrounds text sits on: the app, the surface, the alternate
 *  surface and the hover fill, the translucent ones laid over the
 *  surface where rows and cards draw them. */
export function themeBackgrounds(colors: ThemeColors): Record<'app' | 'surface' | 'surfaceAlt' | 'surfaceHover', Rgb> {
  const surface = parseColour(colors.surface)
  return {
    app: parseColour(colors.app),
    surface,
    surfaceAlt: parseColour(colors.surfaceAlt, surface),
    surfaceHover: parseColour(colors.surfaceHover, surface),
  }
}

/** A theme's colours as they render: the workspace accent applied, the
 *  label colour on the accent picked, and the text versions derived. */
export interface ResolvedTheme {
  colors: ThemeColors
  onAccent: string
  /** `--color-<name>-text` by name, 4.5:1 on every background and on
   *  its own tint laid over each of them. */
  text: Record<(typeof TEXT_COLOURS)[number][0], string>
  /** The accent where it marks state or focus rather than filling: focus
   *  rings, outlines, selected borders. 3:1 on every background (WCAG
   *  1.4.11); `ring-accent`, `outline-accent` and `border-accent` read it. */
  accentUi: string
  /** A text field's resting border, 3:1 on the app, the surface and the
   *  alternate surface, so the field's edge can be found (`border-field`). */
  fieldBorder: string
}

export function resolveTheme(theme: Theme, accentOverride?: string): ResolvedTheme {
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
  // black or white by WCAG contrast against the actual accent
  // fill and its hover fill. This makes the choice Just Work for any
  // accent value, including workspace-branding overrides — and keeps
  // themes whose accent is dark (e.g. epaper's `#000000`) from rendering
  // invisible black-on-black labels, which was the symptom that
  // surfaced this whole token.
  const onAccent = colors.accentForeground ?? pickAccentForeground(colors.accent, colors.accentHover)

  const backgrounds = Object.values(themeBackgrounds(colors))
  const text = Object.fromEntries(
    TEXT_COLOURS.map(([name, key, muted]) => {
      const tint = colors[muted] as string
      const fills = [...backgrounds, ...backgrounds.map((under) => parseColour(tint, under))]
      return [name, readableOn(colors[key] as string, fills, 4.5, theme.meta.isDark)]
    }),
  ) as ResolvedTheme['text']

  const ui = (colour: string, fills: Rgb[]) => readableOn(colour, fills, 3, theme.meta.isDark)
  const { surfaceHover: _hover, ...still } = themeBackgrounds(colors)
  const accentUi = ui(colors.accent, backgrounds)
  const fieldBorder = ui(colors.strong, Object.values(still))

  return { colors, onAccent, text, accentUi, fieldBorder }
}

/**
 * Converts theme colors to CSS variables and injects them into the DOM
 */
export function applyTheme(theme: Theme, accentOverride?: string): void {
  const { colors, onAccent, text, accentUi, fieldBorder } = resolveTheme(theme, accentOverride)

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
  --color-accent-ui: ${accentUi};
  --color-field-border: ${fieldBorder};

  /* Status colors */
  --color-status-success: ${colors.success};
  --color-status-success-muted: ${colors.successMuted};
  --color-status-error: ${colors.error};
  --color-status-error-muted: ${colors.errorMuted};
  --color-status-warning: ${colors.warning};
  --color-status-warning-muted: ${colors.warningMuted};
  --color-status-info: ${colors.info};
  --color-status-info-muted: ${colors.infoMuted};
  --color-overdue: ${overdueColor(theme)};

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

  /* Text versions of the colours above (resolveTheme) */
${Object.entries(text).map(([name, value]) => `  --color-${name}-text: ${value};`).join('\n')}

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
    // Nonced: Tauri's runtime CSP nonce makes the browser ignore the
    // `'unsafe-inline'` we configure, so a bare <style> is blocked and every
    // token below silently fails to apply. See utils/cspNonce.
    styleEl = createNoncedStyleElement(STYLE_ID)
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

  // Cache the resolved app background for the next page load. On the web,
  // the pre-mount cover (`public/splash.js`) reads it so the cover matches
  // the user's theme before any Vue has run; the mobile app's launch
  // follows the phone's light/dark setting instead, like its native launch
  // screen. Mirrors the `nosdesk_branding_cache` pattern read by
  // `branding-init.js`. Best-effort: localStorage can throw in private
  // mode, and a stale cache only costs one off-theme frame.
  try {
    localStorage.setItem('nosdesk_launch_theme', JSON.stringify({ app: colors.app }))
  } catch {
    // ignore: splash falls back to prefers-color-scheme + dark brand
  }
}

/**
 * Pick `#000000` or `#ffffff` for text rendered on top of `accent`,
 * whichever holds the higher WCAG contrast on both the accent fill and
 * its hover fill (a button keeps its label while hovered).
 *
 * The brand orange `#FF6B1A` scores 7.4:1 with black vs 2.85:1
 * with white — picks black. Epaper's `#000000` accent scores
 * 21:1 with white — picks white. Every theme's accent gets an
 * AA-passing foreground without per-theme bookkeeping.
 */
function pickAccentForeground(accent: string, hover: string): string {
  let fills: Rgb[]
  try {
    fills = [parseColour(accent), parseColour(hover)]
  } catch {
    return '#000000' // bail to default on malformed input
  }
  const worst = (fg: Rgb) => Math.min(...fills.map((fill) => contrast(fg, fill)))
  return worst([1, 1, 1]) > worst([0, 0, 0]) ? '#ffffff' : '#000000'
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
