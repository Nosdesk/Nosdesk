import type { Theme } from '../types'

/**
 * Slate Theme
 *
 * The slate-toned dark theme that used to be Nosdesk's default `dark`
 * before the brand spec moved to a near-black base. Kept as a preset
 * for users who prefer the cooler blue-grey palette over true black.
 *
 * Background hierarchy is built on Tailwind's slate-900/800/700
 * family. The colour temperature is meaningfully cooler than the
 * brand-spec `dark` theme — slate has a perceptible blue tint that
 * some find easier on the eyes than near-black for extended sessions.
 *
 * Accent diverges from brand orange. Cyan-teal pairs cool-on-cool
 * with the slate base (the Solarized / VS Code Dark+ family
 * pairing) — monochromatic, low visual fatigue. The user opting
 * into Slate has already signalled "I prefer cool tones over the
 * brand-default warm orange"; honouring that across the accent
 * keeps the theme internally consistent.
 */
export const slateTheme: Theme = {
  meta: {
    id: 'slate',
    name: 'Slate',
    description: 'Cool slate-toned dark theme with brand orange accent',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds — slate-900/800/700 family
    app: '#0f172a',
    surface: '#1e293b',
    surfaceAlt: '#334155',
    surfaceHover: 'rgba(51, 65, 85, 0.5)',

    // Borders — subtle slate in dark mode
    default: '#334155',
    subtle: '#293548',
    strong: '#475569',

    // Text
    primary: '#f9fafb',
    secondary: '#cbd5e1',
    tertiary: '#94a3b8',

    // Accent — cyan teal (Tailwind cyan-500 family), deliberately
    // diverging from the brand orange. Cool-on-cool with the slate
    // base. Hover steps UP to cyan-400 (#22D3EE) so the lift reads
    // on the dark surface; WCAG: cyan-500 on slate-900 is 8.4:1 AAA,
    // hover 9.7:1 AAA.
    accent: '#06B6D4',
    accentHover: '#22D3EE',
    accentMuted: 'rgba(6, 182, 212, 0.22)',

    // Status — brighter for dark mode
    success: '#00C951',
    successMuted: 'rgba(0, 201, 81, 0.2)',
    error: '#EF4444',
    errorMuted: 'rgba(239, 68, 68, 0.2)',
    warning: '#F59E0B',
    warningMuted: 'rgba(245, 158, 11, 0.2)',
    info: '#3B82F6',
    infoMuted: 'rgba(59, 130, 246, 0.2)',

    // Ticket status — brighter for dark mode
    statusOpen: '#FBBF24',
    statusOpenMuted: 'rgba(251, 191, 36, 0.2)',
    statusInProgress: '#60A5FA',
    statusInProgressMuted: 'rgba(96, 165, 250, 0.2)',
    statusClosed: '#34D399',
    statusClosedMuted: 'rgba(52, 211, 153, 0.2)',

    // Priority — brighter for dark mode
    priorityHigh: '#F87171',
    priorityHighMuted: 'rgba(248, 113, 113, 0.2)',
    priorityMedium: '#FBBF24',
    priorityMediumMuted: 'rgba(251, 191, 36, 0.2)',
    priorityLow: '#34D399',
    priorityLowMuted: 'rgba(52, 211, 153, 0.2)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.3)',
    shadowLight: 'rgba(255, 255, 255, 0.05)',

    // Syntax highlighting
    syntax: {
      comment: '#64748b',
      keyword: '#a78bfa',
      string: '#34d399',
      number: '#fbbf24',
      function: '#60a5fa',
      variable: '#f1f5f9',
      type: '#f472b6',
      operator: '#94a3b8',
    },
  },
}
