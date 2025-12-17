import type { Theme } from '../types'

/**
 * Nord Theme
 *
 * An arctic, north-bluish color palette.
 * Based on: https://www.nordtheme.com/
 */
export const nordTheme: Theme = {
  meta: {
    id: 'nord',
    name: 'Nord',
    author: 'Arctic Ice Studio',
    description: 'An arctic, north-bluish color palette',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Polar Night (backgrounds)
    app: '#2e3440',
    surface: '#3b4252',
    surfaceAlt: '#434c5e',
    surfaceHover: '#4c566a',

    // Borders
    default: '#4c566a',
    subtle: '#3b4252',
    strong: '#5e6779',

    // Snow Storm (text)
    primary: '#eceff4',
    secondary: '#e5e9f0',
    tertiary: '#d8dee9',

    // Frost (accent - Nord blue)
    accent: '#88c0d0',
    accentHover: '#81a1c1',
    accentMuted: 'rgba(136, 192, 208, 0.2)',

    // Aurora (status colors)
    success: '#a3be8c',
    successMuted: 'rgba(163, 190, 140, 0.2)',
    error: '#bf616a',
    errorMuted: 'rgba(191, 97, 106, 0.2)',
    warning: '#ebcb8b',
    warningMuted: 'rgba(235, 203, 139, 0.2)',
    info: '#5e81ac',
    infoMuted: 'rgba(94, 129, 172, 0.2)',

    // Ticket status
    statusOpen: '#ebcb8b',
    statusOpenMuted: 'rgba(235, 203, 139, 0.2)',
    statusInProgress: '#81a1c1',
    statusInProgressMuted: 'rgba(129, 161, 193, 0.2)',
    statusClosed: '#a3be8c',
    statusClosedMuted: 'rgba(163, 190, 140, 0.2)',

    // Priority
    priorityHigh: '#bf616a',
    priorityHighMuted: 'rgba(191, 97, 106, 0.2)',
    priorityMedium: '#ebcb8b',
    priorityMediumMuted: 'rgba(235, 203, 139, 0.2)',
    priorityLow: '#a3be8c',
    priorityLowMuted: 'rgba(163, 190, 140, 0.2)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.3)',
    shadowLight: 'rgba(255, 255, 255, 0.05)',

    // Syntax highlighting
    syntax: {
      comment: '#616e88',
      keyword: '#81a1c1',
      string: '#a3be8c',
      number: '#b48ead',
      function: '#88c0d0',
      variable: '#d8dee9',
      type: '#8fbcbb',
      operator: '#81a1c1',
    },
  },
}
