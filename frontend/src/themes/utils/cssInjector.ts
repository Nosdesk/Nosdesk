import type { Theme, ThemeColors } from '../types'
import { lightTheme } from '../base/light'
import { darkTheme } from '../base/dark'
import { contrast, isColour, parseColour, readableOn, solidFor, toHex, type Rgb } from './readable'
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
 * in chips (null: that text sits on plain backgrounds only).
 * `text-status-error`, `text-accent` and the rest read the
 * `--color-<name>-text` version of each (see `--text-color-*` in
 * main.css), derived here per theme. Accent text takes the lightest
 * colour that reads on the backgrounds, so it stays an orange rather
 * than a brown; `text-accent-on-tint` is the one for accent text on an
 * accent tint (an active menu item, an accent chip).
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
  ['accent', 'accent', null],
  ['accent-hover', 'accentHover', null],
  ['accent-on-tint', 'accent', 'accentMuted'],
] as const satisfies ReadonlyArray<readonly [string, keyof ThemeColors, keyof ThemeColors | null]>

/** The status colours that also fill buttons and badges with a label on
 *  them (`bg-status-error-solid text-on-status-error`). */
export const SOLID_COLOURS = [
  ['status-success', 'success'],
  ['status-error', 'error'],
  ['status-warning', 'warning'],
  ['status-info', 'info'],
] as const satisfies ReadonlyArray<readonly [string, keyof ThemeColors]>

/** The status and priority colours that also mark icons and glyphs.
 *  `text-<name>-ui` is the colour at 3:1 (WCAG 1.4.11), lighter than its
 *  text version, for an icon that carries meaning. */
export const UI_COLOURS = TEXT_COLOURS.filter(([name]) => !name.startsWith('accent'))

/** The opacities components lay a colour over a background at, besides
 *  its muted token: `bg-status-error/10`, `/15`, `/20`. Text and icons
 *  in that colour are held to their ratio on all of them. */
export const TINT_ALPHAS = [0.1, 0.15, 0.2] as const

/** Every tint of `colour` its own text can sit on: its muted token and
 *  each of TINT_ALPHAS, laid over each background. */
export function tintsOf(colour: string, muted: string, backgrounds: Rgb[]): Rgb[] {
  const solid = parseColour(colour)
  return backgrounds.flatMap((under) => [
    parseColour(muted, under),
    ...TINT_ALPHAS.map((a) => solid.map((v, i) => v * a + under[i] * (1 - a)) as Rgb),
  ])
}

/** Merged has no theme colour of its own: every theme takes the base
 *  light or dark one (main.css), and its text version is derived here
 *  against the theme's own backgrounds like the rest. */
export const MERGED = {
  light: { fill: '#6366F1', tint: 'rgba(99, 102, 241, 0.15)' },
  dark: { fill: '#818CF8', tint: 'rgba(129, 140, 248, 0.2)' },
} as const

/** The backgrounds text sits on: the app, the surface, the alternate
 *  surface and the hover fill, the translucent ones laid over the
 *  surface where rows and cards draw them, and the hover fill over the
 *  alternate surface. */
export function themeBackgrounds(
  colors: ThemeColors,
): Record<'app' | 'surface' | 'surfaceAlt' | 'surfaceHover' | 'surfaceHoverOnAlt', Rgb> {
  const surface = parseColour(colors.surface)
  const surfaceAlt = parseColour(colors.surfaceAlt, surface)
  return {
    app: parseColour(colors.app),
    surface,
    surfaceAlt,
    surfaceHover: parseColour(colors.surfaceHover, surface),
    // A hovered alternate row: the hover fill laid over the alternate.
    surfaceHoverOnAlt: parseColour(colors.surfaceHover, surfaceAlt),
  }
}

/** A theme's colours as they render: the workspace accent applied, the
 *  label colour on the accent picked, and the text versions derived. */
export interface ResolvedTheme {
  colors: ThemeColors
  onAccent: string
  /** `--color-<name>-text` by name, 4.5:1 on every background and on
   *  its own tint laid over each of them. */
  text: Record<(typeof TEXT_COLOURS)[number][0] | 'status-merged', string>
  /** `--color-<name>-ui`: the status and priority colours at 3:1 on every
   *  background and tint, for icons and glyphs that carry meaning. */
  ui: Record<(typeof UI_COLOURS)[number][0] | 'status-merged', string>
  /** The accent where it marks state or focus rather than filling: focus
   *  rings, outlines, selected borders, icons. 3:1 on every background
   *  and accent tint (WCAG 1.4.11); `ring-accent`, `outline-accent` and `border-accent` read it. */
  accentUi: string
  /** Filled status buttons and badges: the fill (the status colour, moved
   *  in lightness only if it must) and the black or white label on it,
   *  4.5:1 at rest and at `hover:opacity-90`. */
  solid: Record<(typeof SOLID_COLOURS)[number][0], { fill: string; label: string }>
  /** A text field's resting border, 3:1 on the app, the surface and the
   *  alternate surface, so the field's edge can be found (`border-field`). */
  fieldBorder: string
}

/**
 * Never throws: a theme that fails to resolve would leave the app with
 * no colours at all. A workspace accent the parser cannot read (the
 * backend accepts `#f60`, and colours arrive from outside) is dropped in
 * favour of the theme's own, with a warning; anything else that goes
 * wrong falls back to the theme's colours as written.
 */
export function resolveTheme(theme: Theme, accentOverride?: string): ResolvedTheme {
  let accent: string | undefined
  if (accentOverride) {
    if (isColour(accentOverride)) {
      // One form for the arithmetic below: `#rrggbb`, a translucent
      // accent laid over the surface it is seen on.
      const surface = isColour(theme.colors.surface) ? parseColour(theme.colors.surface) : undefined
      accent = toHex(parseColour(accentOverride, surface))
    } else {
      console.warn(`[theme] ignoring unreadable workspace accent ${JSON.stringify(accentOverride)}`)
    }
  }
  try {
    return resolveColours(theme, accent)
  } catch (error) {
    console.warn(`[theme] could not derive colours for ${theme.meta.id}; using them as written`, error)
    const colors = { ...theme.colors }
    const text = Object.fromEntries(TEXT_COLOURS.map(([name, key]) => [name, colors[key] as string]))
    const mergedFill = (theme.meta.isDark ? MERGED.dark : MERGED.light).fill
    return {
      colors,
      onAccent: colors.accentForeground ?? '#000000',
      text: { ...text, 'status-merged': mergedFill } as ResolvedTheme['text'],
      ui: { ...text, 'status-merged': mergedFill } as ResolvedTheme['ui'],
      accentUi: colors.accent,
      solid: Object.fromEntries(
        SOLID_COLOURS.map(([name, key]) => [name, { fill: colors[key] as string, label: '#ffffff' }]),
      ) as ResolvedTheme['solid'],
      fieldBorder: colors.strong,
    }
  }
}

function resolveColours(theme: Theme, accentOverride?: string): ResolvedTheme {
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
  const merged = theme.meta.isDark ? MERGED.dark : MERGED.light
  const fillsFor = (colour: string, muted: string | null) =>
    muted ? [...backgrounds, ...tintsOf(colour, muted, backgrounds)] : backgrounds
  const derive = (colour: string, muted: string | null, ratio: number) =>
    readableOn(colour, fillsFor(colour, muted), ratio, theme.meta.isDark)
  const text = {
    ...Object.fromEntries(
      TEXT_COLOURS.map(([name, key, muted]) => [name, derive(colors[key] as string, muted && (colors[muted] as string), 4.5)]),
    ),
    'status-merged': derive(merged.fill, merged.tint, 4.5),
  } as ResolvedTheme['text']
  const ui = {
    ...Object.fromEntries(
      UI_COLOURS.map(([name, key, muted]) => [name, derive(colors[key] as string, muted && (colors[muted] as string), 3)]),
    ),
    'status-merged': derive(merged.fill, merged.tint, 3),
  } as ResolvedTheme['ui']

  const { surfaceHover: _hover, surfaceHoverOnAlt: _hoverOnAlt, ...still } = themeBackgrounds(colors)
  // Icons and selected states sit on accent tints too.
  const accentUi = derive(colors.accent, colors.accentMuted, 3)
  const fieldBorder = readableOn(colors.strong, Object.values(still), 3, theme.meta.isDark)
  const { surface, app } = themeBackgrounds(colors)
  const solid = Object.fromEntries(
    SOLID_COLOURS.map(([name, key]) => [name, solidFor(colors[key] as string, [surface, app], 4.5)]),
  ) as ResolvedTheme['solid']

  return { colors, onAccent, text, ui, accentUi, solid, fieldBorder }
}

/**
 * Converts theme colors to CSS variables and injects them into the DOM
 */
export function applyTheme(theme: Theme, accentOverride?: string): void {
  const { colors, onAccent, text, ui, accentUi, solid, fieldBorder } = resolveTheme(theme, accentOverride)

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
${Object.entries(ui).map(([name, value]) => `  --color-${name}-ui: ${value};`).join('\n')}
${Object.entries(solid).map(([name, { fill, label }]) => `  --color-${name}-solid: ${fill};\n  --color-on-${name}: ${label};`).join('\n')}

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
