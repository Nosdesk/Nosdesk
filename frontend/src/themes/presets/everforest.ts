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
    surfaceHover: '#e6e2cc',

    // Borders
    default: '#e0dcc7',
    subtle: '#efebd4',
    strong: '#d4d0bb',

    // Text
    primary: '#5c6a72',
    secondary: '#708089',
    tertiary: '#939f91',

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
    primary: '#d3c6aa',
    secondary: '#a7c080',
    tertiary: '#859289',

    // Accent (Everforest green)
    accent: '#a7c080',
    accentHover: '#83c092',
    accentMuted: 'rgba(167, 192, 128, 0.2)',

    // Status
    success: '#a7c080',
    successMuted: 'rgba(167, 192, 128, 0.2)',
    error: '#e67e80',
    errorMuted: 'rgba(230, 126, 128, 0.2)',
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
