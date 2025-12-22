import type { Theme } from '../types'

/**
 * Nossal Dark Theme
 *
 * Warm orange and gold tones inspired by Nossal branding.
 */
export const nossalDarkTheme: Theme = {
  meta: {
    id: 'nossal-dark',
    name: 'Nossal Dark',
    description: 'Warm orange and gold tones',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds - warm dark greys
    app: '#1a1816',
    surface: '#242120',
    surfaceAlt: '#2e2a28',
    surfaceHover: '#383432',

    // Borders - warm grey tones
    default: '#857B78',
    subtle: '#4a4543',
    strong: '#9a908d',

    // Text
    primary: '#f5f0ed',
    secondary: '#d4ccc7',
    tertiary: '#857B78',

    // Accent - Nossal orange
    accent: '#F37929',
    accentHover: '#f58d47',
    accentMuted: 'rgba(243, 121, 41, 0.2)',

    // Status colors
    success: '#4ade80',
    successMuted: 'rgba(74, 222, 128, 0.2)',
    error: '#f87171',
    errorMuted: 'rgba(248, 113, 113, 0.2)',
    warning: '#FDBC14',
    warningMuted: 'rgba(253, 188, 20, 0.2)',
    info: '#60a5fa',
    infoMuted: 'rgba(96, 165, 250, 0.2)',

    // Ticket status - using Nossal palette
    statusOpen: '#FDBC14',
    statusOpenMuted: 'rgba(253, 188, 20, 0.2)',
    statusInProgress: '#F37929',
    statusInProgressMuted: 'rgba(243, 121, 41, 0.2)',
    statusClosed: '#4ade80',
    statusClosedMuted: 'rgba(74, 222, 128, 0.2)',

    // Priority
    priorityHigh: '#f87171',
    priorityHighMuted: 'rgba(248, 113, 113, 0.2)',
    priorityMedium: '#FDBC14',
    priorityMediumMuted: 'rgba(253, 188, 20, 0.2)',
    priorityLow: '#4ade80',
    priorityLowMuted: 'rgba(74, 222, 128, 0.2)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.4)',
    shadowLight: 'rgba(255, 255, 255, 0.05)',

    // Syntax highlighting - warm tones
    syntax: {
      comment: '#857B78',
      keyword: '#F37929',
      string: '#4ade80',
      number: '#FDBC14',
      function: '#f58d47',
      variable: '#d4ccc7',
      type: '#A97C15',
      operator: '#857B78',
    },
  },
}
