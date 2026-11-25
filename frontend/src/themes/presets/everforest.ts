import type { Theme } from '../types'

/**
 * Everforest Light Theme
 *
 * A green-based color scheme designed to be warm and soft.
 * Based on: https://github.com/sainnhe/everforest
 */
export const everforestLightTheme: Theme = {
  meta: {
    id: 'everforest-light',
    name: 'Everforest Light',
    author: 'sainnhe',
    description: 'Green-based, warm and soft on the eyes',
    isDark: false,
    category: 'builtin',
  },
  colors: {
    // Backgrounds
    app: '#fdf6e3',
    surface: '#fffbef',
    surfaceAlt: '#f4f0d9',
    surfaceHover: '#e3dfc9', // 1.30:1 from surface, 1.17:1 from the alternate surface

    // Borders
    default: '#e0dcc7',
    subtle: '#efebd4',
    strong: '#d4d0bb',

    // Text
    primary: '#46525a', // lifted to 6.0:1 on hover, 7.8:1 on surface
    secondary: '#4b5a63', // lifted to 5.3:1 on hover, 6.9:1 on surface
    tertiary: '#596458', // lifted to 4.6:1 on hover, 6.0:1 on surface

    // Accent (Everforest green)
    accent: '#8da101',
    accentHover: '#a7c080',
    accentMuted: 'rgba(141, 161, 1, 0.15)',

    // Status
    success: '#8da101',
    successMuted: 'rgba(141, 161, 1, 0.15)',
    error: '#f85552',
    errorMuted: 'rgba(248, 85, 82, 0.15)',
    warning: '#dfa000',
    warningMuted: 'rgba(223, 160, 0, 0.15)',
    info: '#3a94c5',
    infoMuted: 'rgba(58, 148, 197, 0.15)',

    // Ticket status
    statusOpen: '#dfa000',
    statusOpenMuted: 'rgba(223, 160, 0, 0.15)',
    statusInProgress: '#3a94c5',
    statusInProgressMuted: 'rgba(58, 148, 197, 0.15)',
    statusClosed: '#8da101',
    statusClosedMuted: 'rgba(141, 161, 1, 0.15)',

    // Priority
    priorityHigh: '#f85552',
    priorityHighMuted: 'rgba(248, 85, 82, 0.15)',
    priorityMedium: '#dfa000',
    priorityMediumMuted: 'rgba(223, 160, 0, 0.15)',
    priorityLow: '#8da101',
    priorityLowMuted: 'rgba(141, 161, 1, 0.15)',

    // Shadows
    shadowDark: 'rgba(92, 106, 114, 0.12)',
    shadowLight: 'rgba(255, 255, 255, 0.8)',

    // Syntax highlighting
    syntax: {
      comment: '#939f91',
      keyword: '#f85552',
      string: '#8da101',
      number: '#df69ba',
      function: '#3a94c5',
      variable: '#5c6a72',
      type: '#35a77c',
      operator: '#f57d26',
    },
  },
}

/**
 * Everforest Dark Theme
 *
 * A green-based color scheme designed to be warm and soft - dark variant.
 */
export const everforestDarkTheme: Theme = {
  meta: {
    id: 'everforest-dark',
    name: 'Everforest Dark',
    author: 'sainnhe',
    description: 'Green-based, warm and soft on the eyes',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds
    app: '#2d353b',
    surface: '#343f44',
    surfaceAlt: '#3d484d',
    surfaceHover: '#475258',

    // Borders
    default: '#475258',
    subtle: '#3d484d',
    strong: '#56635f',

    // Text
    primary: '#ebdec1', // lifted to 6.0:1 on hover, 8.1:1 on surface
    secondary: '#c2dc9a', // lifted to 5.4:1 on hover, 7.2:1 on surface
    tertiary: '#bac8be', // lifted to 4.6:1 on hover, 6.2:1 on surface

    // Accent (Everforest green)
    accent: '#a7c080',
    accentHover: '#83c092',
    accentMuted: 'rgba(167, 192, 128, 0.2)',

    // Status
    success: '#a7c080',
    successMuted: 'rgba(167, 192, 128, 0.2)',
    error: '#e67e80',
    errorMuted: 'rgba(230, 126, 128, 0.2)',
    overdue: '#f1b6b7', // red, lifted to 6.2:1 on surface, 4.6:1 on hover
    warning: '#dbbc7f',
    warningMuted: 'rgba(219, 188, 127, 0.2)',
    info: '#7fbbb3',
    infoMuted: 'rgba(127, 187, 179, 0.2)',

    // Ticket status
    statusOpen: '#dbbc7f',
    statusOpenMuted: 'rgba(219, 188, 127, 0.2)',
    statusInProgress: '#7fbbb3',
    statusInProgressMuted: 'rgba(127, 187, 179, 0.2)',
    statusClosed: '#a7c080',
    statusClosedMuted: 'rgba(167, 192, 128, 0.2)',

    // Priority
    priorityHigh: '#e67e80',
    priorityHighMuted: 'rgba(230, 126, 128, 0.2)',
    priorityMedium: '#dbbc7f',
    priorityMediumMuted: 'rgba(219, 188, 127, 0.2)',
    priorityLow: '#a7c080',
    priorityLowMuted: 'rgba(167, 192, 128, 0.2)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.35)',
    shadowLight: 'rgba(255, 255, 255, 0.03)',

    // Syntax highlighting
    syntax: {
      comment: '#859289',
      keyword: '#e67e80',
      string: '#a7c080',
      number: '#d699b6',
      function: '#7fbbb3',
      variable: '#d3c6aa',
      type: '#83c092',
      operator: '#e69875',
    },
  },
}
