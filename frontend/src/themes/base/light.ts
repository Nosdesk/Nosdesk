import type { Theme } from '../types'

/**
 * Default Light Theme
 *
 * Clean, professional light theme with good contrast and readability.
 * Based on the existing Nosdesk light mode colors.
 */
export const lightTheme: Theme = {
  meta: {
    id: 'light',
    name: 'Light',
    description: 'Clean, professional light theme',
    isDark: false,
    category: 'builtin',
  },
  colors: {
    // Backgrounds
    app: '#f3f4f6',
    surface: '#ffffff',
    surfaceAlt: '#f3f4f6',
    surfaceHover: '#e5e7eb',

    // Borders
    default: '#e5e7eb',
    subtle: '#f3f4f6',
    strong: '#d1d5db',

    // Text
    primary: '#1f2937',
    secondary: '#4b5563',
    tertiary: '#6b7280',

    // Accent (default orange - can be overridden by branding)
    accent: '#EE9902',
    accentHover: '#d68800',
    accentMuted: 'rgba(238, 153, 2, 0.18)',

    // Status
    success: '#00C951',
    successMuted: 'rgba(0, 201, 81, 0.15)',
    error: '#EF4444',
    errorMuted: 'rgba(239, 68, 68, 0.15)',
    warning: '#F59E0B',
    warningMuted: 'rgba(245, 158, 11, 0.15)',
    info: '#3B82F6',
    infoMuted: 'rgba(59, 130, 246, 0.15)',

    // Ticket status
    statusOpen: '#F59E0B',
    statusOpenMuted: 'rgba(245, 158, 11, 0.15)',
    statusInProgress: '#3B82F6',
    statusInProgressMuted: 'rgba(59, 130, 246, 0.15)',
    statusClosed: '#10B981',
    statusClosedMuted: 'rgba(16, 185, 129, 0.15)',

    // Priority
    priorityHigh: '#EF4444',
    priorityHighMuted: 'rgba(239, 68, 68, 0.15)',
    priorityMedium: '#F59E0B',
    priorityMediumMuted: 'rgba(245, 158, 11, 0.15)',
    priorityLow: '#10B981',
    priorityLowMuted: 'rgba(16, 185, 129, 0.15)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.1)',
    shadowLight: 'rgba(255, 255, 255, 0.8)',

    // Syntax highlighting
    syntax: {
      comment: '#6b7280',
      keyword: '#8b5cf6',
      string: '#10b981',
      number: '#f59e0b',
      function: '#3b82f6',
      variable: '#1f2937',
      type: '#ec4899',
      operator: '#6b7280',
    },
  },
}
