import type { Theme } from '../types'

/**
 * Tokyo Night Theme
 *
 * A clean, dark theme that celebrates the lights of downtown Tokyo at night.
 * Based on: https://github.com/enkia/tokyo-night-vscode-theme
 */
export const tokyoNightTheme: Theme = {
  meta: {
    id: 'tokyo-night',
    name: 'Tokyo Night',
    author: 'enkia',
    description: 'A clean dark theme inspired by Tokyo at night',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds
    app: '#1a1b26',
    surface: '#24283b',
    surfaceAlt: '#1f2335',
    surfaceHover: '#292e42',

    // Borders
    default: '#3b4261',
    subtle: '#292e42',
    strong: '#545c7e',

    // Text
    primary: '#c0caf5',
    secondary: '#a9b1d6',
    tertiary: '#565f89',

    // Accent (Tokyo Night blue)
    accent: '#7aa2f7',
    accentHover: '#89b4fa',
    accentMuted: 'rgba(122, 162, 247, 0.2)',

    // Status
    success: '#9ece6a',
    successMuted: 'rgba(158, 206, 106, 0.2)',
    error: '#f7768e',
    errorMuted: 'rgba(247, 118, 142, 0.2)',
    warning: '#e0af68',
    warningMuted: 'rgba(224, 175, 104, 0.2)',
    info: '#7dcfff',
    infoMuted: 'rgba(125, 207, 255, 0.2)',

    // Ticket status
    statusOpen: '#e0af68',
    statusOpenMuted: 'rgba(224, 175, 104, 0.2)',
    statusInProgress: '#7aa2f7',
    statusInProgressMuted: 'rgba(122, 162, 247, 0.2)',
    statusClosed: '#9ece6a',
    statusClosedMuted: 'rgba(158, 206, 106, 0.2)',

    // Priority
    priorityHigh: '#f7768e',
    priorityHighMuted: 'rgba(247, 118, 142, 0.2)',
    priorityMedium: '#e0af68',
    priorityMediumMuted: 'rgba(224, 175, 104, 0.2)',
    priorityLow: '#9ece6a',
    priorityLowMuted: 'rgba(158, 206, 106, 0.2)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.4)',
    shadowLight: 'rgba(255, 255, 255, 0.03)',

    // Syntax highlighting
    syntax: {
      comment: '#565f89',
      keyword: '#bb9af7',
      string: '#9ece6a',
      number: '#ff9e64',
      function: '#7aa2f7',
      variable: '#c0caf5',
      type: '#2ac3de',
      operator: '#89ddff',
    },
  },
}
