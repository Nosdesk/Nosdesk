import type { Theme } from '../types'

/**
 * Dracula Theme
 *
 * A dark theme with vibrant colors, easy on the eyes.
 * Based on: https://draculatheme.com/
 */
export const draculaTheme: Theme = {
  meta: {
    id: 'dracula',
    name: 'Dracula',
    author: 'Zeno Rocha',
    description: 'A dark theme with vibrant colors',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds
    app: '#282a36',
    surface: '#44475a',
    surfaceAlt: '#383a4a',
    surfaceHover: '#4d4f5c',

    // Borders
    default: '#44475a',
    subtle: '#383a4a',
    strong: '#6272a4',

    // Text
    primary: '#f8f8f2',
    secondary: '#e2e2dc',
    tertiary: '#6272a4',

    // Accent (Dracula purple)
    accent: '#bd93f9',
    accentHover: '#caa9fa',
    accentMuted: 'rgba(189, 147, 249, 0.2)',

    // Status
    success: '#50fa7b',
    successMuted: 'rgba(80, 250, 123, 0.2)',
    error: '#ff5555',
    errorMuted: 'rgba(255, 85, 85, 0.2)',
    warning: '#f1fa8c',
    warningMuted: 'rgba(241, 250, 140, 0.2)',
    info: '#8be9fd',
    infoMuted: 'rgba(139, 233, 253, 0.2)',

    // Ticket status
    statusOpen: '#ffb86c',
    statusOpenMuted: 'rgba(255, 184, 108, 0.2)',
    statusInProgress: '#8be9fd',
    statusInProgressMuted: 'rgba(139, 233, 253, 0.2)',
    statusClosed: '#50fa7b',
    statusClosedMuted: 'rgba(80, 250, 123, 0.2)',

    // Priority
    priorityHigh: '#ff5555',
    priorityHighMuted: 'rgba(255, 85, 85, 0.2)',
    priorityMedium: '#ffb86c',
    priorityMediumMuted: 'rgba(255, 184, 108, 0.2)',
    priorityLow: '#50fa7b',
    priorityLowMuted: 'rgba(80, 250, 123, 0.2)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.4)',
    shadowLight: 'rgba(255, 255, 255, 0.03)',

    // Syntax highlighting
    syntax: {
      comment: '#6272a4',
      keyword: '#ff79c6',
      string: '#f1fa8c',
      number: '#bd93f9',
      function: '#50fa7b',
      variable: '#f8f8f2',
      type: '#8be9fd',
      operator: '#ff79c6',
    },
  },
}
