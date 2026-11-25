import type { Theme } from '../types'

/**
 * Gruvbox Light Theme
 *
 * Retro groove color scheme with warm, earthy tones.
 * Based on: https://github.com/morhetz/gruvbox
 */
export const gruvboxLightTheme: Theme = {
  meta: {
    id: 'gruvbox-light',
    name: 'Gruvbox Light',
    author: 'morhetz',
    description: 'Retro groove with warm, earthy tones',
    isDark: false,
    category: 'builtin',
  },
  colors: {
    // Backgrounds
    app: '#fbf1c7',
    surface: '#f9f5d7',
    surfaceAlt: '#ebdbb2',
    surfaceHover: '#d5c4a1',

    // Borders
    default: '#d5c4a1',
    subtle: '#ebdbb2',
    strong: '#bdae93',

    // Text
    primary: '#3c3836',
    secondary: '#4d4642', // lifted to 5.4:1 on hover, 8.4:1 on surface
    tertiary: '#594f48', // lifted to 4.6:1 on hover, 7.2:1 on surface

    // Accent (Gruvbox aqua)
    accent: '#427b58',
    accentHover: '#38714f', // a step darker, so the white label holds 4.5:1 while hovered
    accentMuted: 'rgba(66, 123, 88, 0.15)',

    // Status
    success: '#79740e',
    successMuted: 'rgba(121, 116, 14, 0.15)',
    error: '#9d0006',
    errorMuted: 'rgba(157, 0, 6, 0.15)',
    overdue: '#9d0006', // faded red: 7.8:1 on surface, 5.0:1 on hover
    warning: '#b57614',
    warningMuted: 'rgba(181, 118, 20, 0.15)',
    info: '#076678',
    infoMuted: 'rgba(7, 102, 120, 0.15)',

    // Ticket status
    statusOpen: '#b57614',
    statusOpenMuted: 'rgba(181, 118, 20, 0.15)',
    statusInProgress: '#076678',
    statusInProgressMuted: 'rgba(7, 102, 120, 0.15)',
    statusClosed: '#79740e',
    statusClosedMuted: 'rgba(121, 116, 14, 0.15)',

    // Priority
    priorityHigh: '#9d0006',
    priorityHighMuted: 'rgba(157, 0, 6, 0.15)',
    priorityMedium: '#b57614',
    priorityMediumMuted: 'rgba(181, 118, 20, 0.15)',
    priorityLow: '#79740e',
    priorityLowMuted: 'rgba(121, 116, 14, 0.15)',

    // Shadows
    shadowDark: 'rgba(60, 56, 54, 0.15)',
    shadowLight: 'rgba(255, 255, 255, 0.8)',

    // Syntax highlighting
    syntax: {
      comment: '#928374',
      keyword: '#9d0006',
      string: '#79740e',
      number: '#8f3f71',
      function: '#427b58',
      variable: '#3c3836',
      type: '#076678',
      operator: '#af3a03',
    },
  },
}

/**
 * Gruvbox Dark Theme
 *
 * Retro groove color scheme with warm, earthy tones - dark variant.
 */
export const gruvboxDarkTheme: Theme = {
  meta: {
    id: 'gruvbox-dark',
    name: 'Gruvbox Dark',
    author: 'morhetz',
    description: 'Retro groove with warm, earthy tones',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds
    app: '#282828',
    surface: '#3c3836',
    surfaceAlt: '#504945',
    surfaceHover: '#5c524a', // 1.52:1 from surface, 1.16:1 from the alternate surface

    // Borders
    default: '#504945',
    subtle: '#3c3836',
    strong: '#7c6f64',

    // Text
    primary: '#f4e4ba', // lifted to 6.0:1 on hover, 9.2:1 on surface
    secondary: '#e8d7b2', // lifted to 5.4:1 on hover, 8.2:1 on surface
    tertiary: '#d7c8b1', // lifted to 4.6:1 on hover, 7.1:1 on surface

    // Accent (Gruvbox aqua)
    accent: '#8ec07c',
    accentHover: '#b8bb26',
    accentMuted: 'rgba(142, 192, 124, 0.2)',

    // Status
    success: '#b8bb26',
    successMuted: 'rgba(184, 187, 38, 0.2)',
    error: '#fb4934',
    errorMuted: 'rgba(251, 73, 52, 0.2)',
    overdue: '#fed0ca', // bright red, lifted to 4.7:1 on the light hover
    warning: '#fabd2f',
    warningMuted: 'rgba(250, 189, 47, 0.2)',
    info: '#83a598',
    infoMuted: 'rgba(131, 165, 152, 0.2)',

    // Ticket status
    statusOpen: '#fabd2f',
    statusOpenMuted: 'rgba(250, 189, 47, 0.2)',
    statusInProgress: '#83a598',
    statusInProgressMuted: 'rgba(131, 165, 152, 0.2)',
    statusClosed: '#b8bb26',
    statusClosedMuted: 'rgba(184, 187, 38, 0.2)',

    // Priority
    priorityHigh: '#fb4934',
    priorityHighMuted: 'rgba(251, 73, 52, 0.2)',
    priorityMedium: '#fabd2f',
    priorityMediumMuted: 'rgba(250, 189, 47, 0.2)',
    priorityLow: '#b8bb26',
    priorityLowMuted: 'rgba(184, 187, 38, 0.2)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.4)',
    shadowLight: 'rgba(255, 255, 255, 0.03)',

    // Syntax highlighting
    syntax: {
      comment: '#928374',
      keyword: '#fb4934',
      string: '#b8bb26',
      number: '#d3869b',
      function: '#8ec07c',
      variable: '#ebdbb2',
      type: '#83a598',
      operator: '#fe8019',
    },
  },
}
