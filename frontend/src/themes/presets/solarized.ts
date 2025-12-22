import type { Theme } from '../types'

/**
 * Solarized Light Theme
 *
 * Precision colors for machines and people.
 * Based on: https://ethanschoonover.com/solarized/
 */
export const solarizedLightTheme: Theme = {
  meta: {
    id: 'solarized-light',
    name: 'Solarized Light',
    author: 'Ethan Schoonover',
    description: 'Precision colors designed for readability',
    isDark: false,
    category: 'builtin',
  },
  colors: {
    // Base tones (light mode)
    app: '#fdf6e3',      // base3
    surface: '#eee8d5',  // base2
    surfaceAlt: '#e4ddc8', // slightly darker
    surfaceHover: '#ddd6c1',

    // Borders
    default: '#93a1a1',  // base1
    subtle: '#c5c8c0',
    strong: '#839496',   // base0

    // Text (dark content tones)
    primary: '#073642',   // base02
    secondary: '#586e75', // base01
    tertiary: '#657b83',  // base00

    // Accent (Solarized blue)
    accent: '#268bd2',
    accentHover: '#1a6da3',
    accentMuted: 'rgba(38, 139, 210, 0.15)',

    // Status colors
    success: '#859900',   // green
    successMuted: 'rgba(133, 153, 0, 0.15)',
    error: '#dc322f',     // red
    errorMuted: 'rgba(220, 50, 47, 0.15)',
    warning: '#b58900',   // yellow
    warningMuted: 'rgba(181, 137, 0, 0.15)',
    info: '#2aa198',      // cyan
    infoMuted: 'rgba(42, 161, 152, 0.15)',

    // Ticket status
    statusOpen: '#b58900',
    statusOpenMuted: 'rgba(181, 137, 0, 0.15)',
    statusInProgress: '#268bd2',
    statusInProgressMuted: 'rgba(38, 139, 210, 0.15)',
    statusClosed: '#859900',
    statusClosedMuted: 'rgba(133, 153, 0, 0.15)',

    // Priority
    priorityHigh: '#dc322f',
    priorityHighMuted: 'rgba(220, 50, 47, 0.15)',
    priorityMedium: '#b58900',
    priorityMediumMuted: 'rgba(181, 137, 0, 0.15)',
    priorityLow: '#859900',
    priorityLowMuted: 'rgba(133, 153, 0, 0.15)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.1)',
    shadowLight: 'rgba(255, 255, 255, 0.5)',

    // Syntax highlighting
    syntax: {
      comment: '#93a1a1',
      keyword: '#859900',
      string: '#2aa198',
      number: '#d33682',
      function: '#268bd2',
      variable: '#657b83',
      type: '#b58900',
      operator: '#586e75',
    },
  },
}

/**
 * Solarized Dark Theme
 *
 * Precision colors for machines and people.
 * Based on: https://ethanschoonover.com/solarized/
 */
export const solarizedDarkTheme: Theme = {
  meta: {
    id: 'solarized-dark',
    name: 'Solarized Dark',
    author: 'Ethan Schoonover',
    description: 'Precision colors designed for readability',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Base tones (dark mode)
    app: '#002b36',      // base03
    surface: '#073642',  // base02
    surfaceAlt: '#0a4351',
    surfaceHover: '#0d4f5e',

    // Borders
    default: '#586e75',  // base01
    subtle: '#073642',
    strong: '#657b83',   // base00

    // Text (light content tones)
    primary: '#fdf6e3',   // base3
    secondary: '#eee8d5', // base2
    tertiary: '#93a1a1',  // base1

    // Accent (Solarized blue)
    accent: '#268bd2',
    accentHover: '#4ca2e0',
    accentMuted: 'rgba(38, 139, 210, 0.25)',

    // Status colors
    success: '#859900',   // green
    successMuted: 'rgba(133, 153, 0, 0.25)',
    error: '#dc322f',     // red
    errorMuted: 'rgba(220, 50, 47, 0.25)',
    warning: '#b58900',   // yellow
    warningMuted: 'rgba(181, 137, 0, 0.25)',
    info: '#2aa198',      // cyan
    infoMuted: 'rgba(42, 161, 152, 0.25)',

    // Ticket status
    statusOpen: '#b58900',
    statusOpenMuted: 'rgba(181, 137, 0, 0.25)',
    statusInProgress: '#268bd2',
    statusInProgressMuted: 'rgba(38, 139, 210, 0.25)',
    statusClosed: '#859900',
    statusClosedMuted: 'rgba(133, 153, 0, 0.25)',

    // Priority
    priorityHigh: '#dc322f',
    priorityHighMuted: 'rgba(220, 50, 47, 0.25)',
    priorityMedium: '#b58900',
    priorityMediumMuted: 'rgba(181, 137, 0, 0.25)',
    priorityLow: '#859900',
    priorityLowMuted: 'rgba(133, 153, 0, 0.25)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.4)',
    shadowLight: 'rgba(255, 255, 255, 0.05)',

    // Syntax highlighting
    syntax: {
      comment: '#586e75',
      keyword: '#859900',
      string: '#2aa198',
      number: '#d33682',
      function: '#268bd2',
      variable: '#93a1a1',
      type: '#b58900',
      operator: '#657b83',
    },
  },
}
