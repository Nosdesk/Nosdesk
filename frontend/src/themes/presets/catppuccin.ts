import type { Theme } from '../types'

/**
 * Catppuccin Latte Theme (Light)
 *
 * Soothing pastel theme for the high-spirited!
 * Based on: https://github.com/catppuccin/catppuccin
 */
export const catppuccinLatteTheme: Theme = {
  meta: {
    id: 'catppuccin-latte',
    name: 'Catppuccin Latte',
    author: 'Catppuccin',
    description: 'Soothing pastel theme - light variant',
    isDark: false,
    category: 'builtin',
  },
  colors: {
    // Backgrounds
    app: '#eff1f5',
    surface: '#e6e9ef',
    surfaceAlt: '#dce0e8',
    surfaceHover: '#ccd0da',

    // Borders
    default: '#ccd0da',
    subtle: '#dce0e8',
    strong: '#bcc0cc',

    // Text
    primary: '#4c4f69',
    secondary: '#5c5f77',
    tertiary: '#6c6f85',

    // Accent (Catppuccin mauve)
    accent: '#8839ef',
    accentHover: '#7c3aed',
    accentMuted: 'rgba(136, 57, 239, 0.15)',

    // Status
    success: '#40a02b',
    successMuted: 'rgba(64, 160, 43, 0.15)',
    error: '#d20f39',
    errorMuted: 'rgba(210, 15, 57, 0.15)',
    warning: '#df8e1d',
    warningMuted: 'rgba(223, 142, 29, 0.15)',
    info: '#1e66f5',
    infoMuted: 'rgba(30, 102, 245, 0.15)',

    // Ticket status
    statusOpen: '#df8e1d',
    statusOpenMuted: 'rgba(223, 142, 29, 0.15)',
    statusInProgress: '#1e66f5',
    statusInProgressMuted: 'rgba(30, 102, 245, 0.15)',
    statusClosed: '#40a02b',
    statusClosedMuted: 'rgba(64, 160, 43, 0.15)',

    // Priority
    priorityHigh: '#d20f39',
    priorityHighMuted: 'rgba(210, 15, 57, 0.15)',
    priorityMedium: '#df8e1d',
    priorityMediumMuted: 'rgba(223, 142, 29, 0.15)',
    priorityLow: '#40a02b',
    priorityLowMuted: 'rgba(64, 160, 43, 0.15)',

    // Shadows
    shadowDark: 'rgba(76, 79, 105, 0.1)',
    shadowLight: 'rgba(255, 255, 255, 0.8)',

    // Syntax highlighting
    syntax: {
      comment: '#9ca0b0',
      keyword: '#8839ef',
      string: '#40a02b',
      number: '#fe640b',
      function: '#1e66f5',
      variable: '#4c4f69',
      type: '#179299',
      operator: '#04a5e5',
    },
  },
}

/**
 * Catppuccin Mocha Theme (Dark)
 *
 * Soothing pastel theme for the high-spirited! - darkest variant.
 */
export const catppuccinMochaTheme: Theme = {
  meta: {
    id: 'catppuccin-mocha',
    name: 'Catppuccin Mocha',
    author: 'Catppuccin',
    description: 'Soothing pastel theme - dark variant',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds
    app: '#1e1e2e',
    surface: '#313244',
    surfaceAlt: '#45475a',
    surfaceHover: '#585b70',

    // Borders
    default: '#45475a',
    subtle: '#313244',
    strong: '#6c7086',

    // Text
    primary: '#cdd6f4',
    secondary: '#bac2de',
    tertiary: '#a6adc8',

    // Accent (Catppuccin mauve)
    accent: '#cba6f7',
    accentHover: '#b4befe',
    accentMuted: 'rgba(203, 166, 247, 0.2)',

    // Status
    success: '#a6e3a1',
    successMuted: 'rgba(166, 227, 161, 0.2)',
    error: '#f38ba8',
    errorMuted: 'rgba(243, 139, 168, 0.2)',
    warning: '#f9e2af',
    warningMuted: 'rgba(249, 226, 175, 0.2)',
    info: '#89b4fa',
    infoMuted: 'rgba(137, 180, 250, 0.2)',

    // Ticket status
    statusOpen: '#f9e2af',
    statusOpenMuted: 'rgba(249, 226, 175, 0.2)',
    statusInProgress: '#89b4fa',
    statusInProgressMuted: 'rgba(137, 180, 250, 0.2)',
    statusClosed: '#a6e3a1',
    statusClosedMuted: 'rgba(166, 227, 161, 0.2)',

    // Priority
    priorityHigh: '#f38ba8',
    priorityHighMuted: 'rgba(243, 139, 168, 0.2)',
    priorityMedium: '#f9e2af',
    priorityMediumMuted: 'rgba(249, 226, 175, 0.2)',
    priorityLow: '#a6e3a1',
    priorityLowMuted: 'rgba(166, 227, 161, 0.2)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.4)',
    shadowLight: 'rgba(255, 255, 255, 0.03)',

    // Syntax highlighting
    syntax: {
      comment: '#6c7086',
      keyword: '#cba6f7',
      string: '#a6e3a1',
      number: '#fab387',
      function: '#89b4fa',
      variable: '#cdd6f4',
      type: '#94e2d5',
      operator: '#89dceb',
    },
  },
}
