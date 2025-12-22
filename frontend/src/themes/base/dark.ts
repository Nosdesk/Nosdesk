import type { Theme } from '../types'

/**
 * Default Dark Theme
 *
 * Modern dark theme with slate tones for reduced eye strain.
 * Based on the existing Nosdesk dark mode colors.
 */
export const darkTheme: Theme = {
  meta: {
    id: 'dark',
    name: 'Dark',
    description: 'Modern dark theme with slate tones',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds
    app: '#0f172a',
    surface: '#1e293b',
    surfaceAlt: '#334155',
    surfaceHover: 'rgba(51, 65, 85, 0.5)',

    // Borders
    default: '#334155',
    subtle: '#293548',
    strong: '#475569',

    // Text
    primary: '#f9fafb',
    secondary: '#cbd5e1',
    tertiary: '#94a3b8',

    // Accent (default orange - can be overridden by branding)
    accent: '#EE9902',
    accentHover: '#ffaa22',
    accentMuted: 'rgba(238, 153, 2, 0.22)',

    // Status
    success: '#00C951',
    successMuted: 'rgba(0, 201, 81, 0.2)',
    error: '#EF4444',
    errorMuted: 'rgba(239, 68, 68, 0.2)',
    warning: '#F59E0B',
    warningMuted: 'rgba(245, 158, 11, 0.2)',
    info: '#3B82F6',
    infoMuted: 'rgba(59, 130, 246, 0.2)',

    // Ticket status
    statusOpen: '#FBBF24',
    statusOpenMuted: 'rgba(251, 191, 36, 0.2)',
    statusInProgress: '#60A5FA',
    statusInProgressMuted: 'rgba(96, 165, 250, 0.2)',
    statusClosed: '#34D399',
    statusClosedMuted: 'rgba(52, 211, 153, 0.2)',

    // Priority
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
