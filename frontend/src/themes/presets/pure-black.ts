import type { Theme } from '../types'

/**
 * OLED Theme
 *
 * Pure black backgrounds optimized for OLED displays.
 * Maximizes battery life and provides deep contrast.
 */
export const pureBlackTheme: Theme = {
  meta: {
    id: 'pure-black',
    name: 'Pure Black',
    description: 'Pure black theme for OLED displays',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds - pure black for OLED
    app: '#000000',
    surface: '#000000',
    surfaceAlt: '#121212',
    surfaceHover: '#1a1a1a',

    // Borders - subtle dark grey
    default: '#2a2a2a',
    subtle: '#1a1a1a',
    strong: '#3a3a3a',

    // Text - high contrast white
    primary: '#ffffff',
    secondary: '#b3b3b3',
    tertiary: '#808080',

    // Accent (default orange - can be overridden by branding)
    accent: '#EE9902',
    accentHover: '#ffaa22',
    accentMuted: 'rgba(238, 153, 2, 0.25)',

    // Status
    success: '#00C951',
    successMuted: 'rgba(0, 201, 81, 0.25)',
    error: '#EF4444',
    errorMuted: 'rgba(239, 68, 68, 0.25)',
    warning: '#F59E0B',
    warningMuted: 'rgba(245, 158, 11, 0.25)',
    info: '#3B82F6',
    infoMuted: 'rgba(59, 130, 246, 0.25)',

    // Ticket status
    statusOpen: '#FBBF24',
    statusOpenMuted: 'rgba(251, 191, 36, 0.25)',
    statusInProgress: '#60A5FA',
    statusInProgressMuted: 'rgba(96, 165, 250, 0.25)',
    statusClosed: '#34D399',
    statusClosedMuted: 'rgba(52, 211, 153, 0.25)',

    // Priority
    priorityHigh: '#F87171',
    priorityHighMuted: 'rgba(248, 113, 113, 0.25)',
    priorityMedium: '#FBBF24',
    priorityMediumMuted: 'rgba(251, 191, 36, 0.25)',
    priorityLow: '#34D399',
    priorityLowMuted: 'rgba(52, 211, 153, 0.25)',

    // Shadows - minimal on pure black
    shadowDark: 'rgba(0, 0, 0, 0.5)',
    shadowLight: 'rgba(255, 255, 255, 0.03)',

    // Syntax highlighting
    syntax: {
      comment: '#666666',
      keyword: '#a78bfa',
      string: '#34d399',
      number: '#fbbf24',
      function: '#60a5fa',
      variable: '#f1f5f9',
      type: '#f472b6',
      operator: '#808080',
    },
  },
}
