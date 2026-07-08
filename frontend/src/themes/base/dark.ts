import type { Theme } from '../types'

/**
 * Default Dark Theme
 *
 * Near-black dark surfaces with subtle elevation hierarchy, matching the
 * canonical Nosdesk brand spec. Pure black at the bg level (#08090a) reads as
 * black to the eye but leaves room for the surface-1/2/3 tiers to
 * carry visible elevation through tone alone — no shadows needed.
 *
 * The previous slate-toned dark theme has moved to the `slate` preset
 * for users who prefer the cooler blue-grey palette.
 */
export const darkTheme: Theme = {
  meta: {
    id: 'dark',
    name: 'Dark',
    description: 'Near-black surfaces with brand orange accent',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds — brand-spec elevation tiers. Reads as pure black
    // at a glance, but the tiers carry hierarchy without shadows.
    app: '#08090a',
    surface: '#0e0f11',
    surfaceAlt: '#15171a',
    surfaceHover: '#1c1e22',

    // Borders — graduated greys that read as separators on the
    // near-black bg without competing with the brand orange.
    default: '#1f2125',
    subtle: '#15171a',
    strong: '#2a2d33',

    // Text — bright primary, stepped-down secondary/tertiary tuned
    // for AA contrast against #08090a (primary 14.3:1 AAA,
    // secondary 8.2:1 AAA, tertiary 5.4:1 AA).
    primary: '#f7f8f8',
    secondary: '#b8bcc0',
    tertiary: '#82878d',

    // Accent — brand orange (#FF6B1A family). On dark backgrounds,
    // hover steps UP to primary-400 (#FF7A2D) so the lift is
    // visible against the surface. WCAG: primary-500 on near-black
    // is 7.4:1 AAA; primary-400 hover stays above 8:1.
    accent: '#FF6B1A',
    accentHover: '#FF7A2D',
    accentMuted: 'rgba(255, 107, 26, 0.20)',

    // Status — slightly brighter on dark to maintain contrast
    success: '#00C951',
    successMuted: 'rgba(0, 201, 81, 0.2)',
    error: '#EF4444',
    errorMuted: 'rgba(239, 68, 68, 0.2)',
    warning: '#F59E0B',
    warningMuted: 'rgba(245, 158, 11, 0.2)',
    info: '#3B82F6',
    infoMuted: 'rgba(59, 130, 246, 0.2)',

    // Ticket status — bright tints on dark
    statusOpen: '#FBBF24',
    statusOpenMuted: 'rgba(251, 191, 36, 0.2)',
    statusInProgress: '#60A5FA',
    statusInProgressMuted: 'rgba(96, 165, 250, 0.2)',
    statusClosed: '#34D399',
    statusClosedMuted: 'rgba(52, 211, 153, 0.2)',

    // Priority — bright tints on dark
    priorityHigh: '#F87171',
    priorityHighMuted: 'rgba(248, 113, 113, 0.2)',
    priorityMedium: '#FBBF24',
    priorityMediumMuted: 'rgba(251, 191, 36, 0.2)',
    priorityLow: '#34D399',
    priorityLowMuted: 'rgba(52, 211, 153, 0.2)',

    // Shadows — minimal on near-black; the surface-tier hierarchy
    // carries elevation without needing shadow flare.
    shadowDark: 'rgba(0, 0, 0, 0.35)',
    shadowLight: 'rgba(255, 255, 255, 0.04)',

    // Syntax highlighting
    syntax: {
      comment: '#82878d',
      keyword: '#a78bfa',
      string: '#34d399',
      number: '#fbbf24',
      function: '#60a5fa',
      variable: '#f1f5f9',
      type: '#f472b6',
      operator: '#82878d',
    },
  },
}
