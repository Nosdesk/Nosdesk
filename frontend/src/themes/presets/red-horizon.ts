import type { Theme } from '../types'

/**
 * Red Horizon Theme
 *
 * Inspired by the Toshiba T3100e gas-plasma display.
 * Warm amber/orange monochrome on deep black - that classic
 * 80s/90s portable workstation aesthetic.
 */
export const redHorizonTheme: Theme = {
  meta: {
    id: 'red-horizon',
    name: 'Red Horizon',
    description: 'Reddish-orange gas-plasma display',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds - deep blacks with warm red undertone
    app: '#080200',
    surface: '#0c0400',
    surfaceAlt: '#140a00',
    surfaceHover: '#1c0e00',

    // Borders - dark burnt orange
    default: '#3d1800',
    subtle: '#2a1000',
    strong: '#502200',

    // Text - reddish-orange for that warm plasma glow
    primary: '#ff8844',      // Bright reddish-orange - main text
    secondary: '#dd6622',    // Medium burnt orange
    tertiary: '#aa4400',     // Dim reddish-orange

    // Accent - vivid red-orange
    accent: '#ff6622',
    accentHover: '#ff8844',
    accentMuted: 'rgba(255, 102, 34, 0.25)',

    // Status colors - red-orange spectrum
    success: '#dd7700',
    successMuted: 'rgba(220, 119, 0, 0.3)',
    error: '#ff4400',
    errorMuted: 'rgba(255, 68, 0, 0.4)',
    warning: '#ffaa22',
    warningMuted: 'rgba(255, 170, 34, 0.3)',
    info: '#ee6600',
    infoMuted: 'rgba(238, 102, 0, 0.3)',

    // Ticket status - red-orange intensity levels
    statusOpen: '#ff9933',
    statusOpenMuted: 'rgba(255, 153, 51, 0.3)',
    statusInProgress: '#ee6600',
    statusInProgressMuted: 'rgba(238, 102, 0, 0.3)',
    statusClosed: '#884400',
    statusClosedMuted: 'rgba(136, 68, 0, 0.4)',

    // Priority - red-orange brightness levels
    priorityHigh: '#ff7733',
    priorityHighMuted: 'rgba(255, 119, 51, 0.35)',
    priorityMedium: '#dd5522',
    priorityMediumMuted: 'rgba(220, 85, 34, 0.3)',
    priorityLow: '#aa4400',
    priorityLowMuted: 'rgba(170, 68, 0, 0.3)',

    // Shadows - warm red-orange glow
    shadowDark: 'rgba(0, 0, 0, 0.7)',
    shadowLight: 'rgba(255, 102, 34, 0.15)',

    // Syntax highlighting - red-orange monochrome
    syntax: {
      comment: '#884400',
      keyword: '#ff8844',
      string: '#dd7744',
      number: '#ffaa44',
      function: '#ff6622',
      variable: '#dd5522',
      type: '#ee7733',
      operator: '#aa5500',
    },
  },
}
