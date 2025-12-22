import type { Theme } from '../types'

/**
 * Red Horizon Theme
 *
 * Mars colony terminal aesthetic - red/orange plasma CRT display
 * with that warm phosphor glow of a dusty red planet outpost.
 */
export const redHorizonTheme: Theme = {
  meta: {
    id: 'red-horizon',
    name: 'Red Horizon',
    description: 'Mars terminal plasma display',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds - true blacks with subtle red tint
    app: '#080101',
    surface: '#0f0303',
    surfaceAlt: '#180606',
    surfaceHover: '#220909',

    // Borders - dark blood red
    default: '#3a0a0a',
    subtle: '#280707',
    strong: '#4c0d0d',

    // Text - bright red for contrast on dark backgrounds
    primary: '#ff1a1a',
    secondary: '#cc1111',
    tertiary: '#881010',

    // Accent - vivid red
    accent: '#ee0000',
    accentHover: '#ff2222',
    accentMuted: 'rgba(220, 0, 0, 0.25)',

    // Status colors - deep reds
    success: '#cc2200',
    successMuted: 'rgba(180, 30, 0, 0.3)',
    error: '#ff0000',
    errorMuted: 'rgba(180, 0, 0, 0.4)',
    warning: '#dd4400',
    warningMuted: 'rgba(200, 60, 0, 0.3)',
    info: '#cc1100',
    infoMuted: 'rgba(180, 15, 0, 0.3)',

    // Ticket status - blood red spectrum
    statusOpen: '#dd3300',
    statusOpenMuted: 'rgba(200, 45, 0, 0.3)',
    statusInProgress: '#cc1100',
    statusInProgressMuted: 'rgba(180, 15, 0, 0.3)',
    statusClosed: '#660000',
    statusClosedMuted: 'rgba(80, 0, 0, 0.4)',

    // Priority - intensity of red
    priorityHigh: '#ff0000',
    priorityHighMuted: 'rgba(200, 0, 0, 0.35)',
    priorityMedium: '#cc2200',
    priorityMediumMuted: 'rgba(180, 30, 0, 0.3)',
    priorityLow: '#992200',
    priorityLowMuted: 'rgba(140, 30, 0, 0.3)',

    // Shadows - deep red glow
    shadowDark: 'rgba(0, 0, 0, 0.7)',
    shadowLight: 'rgba(200, 0, 0, 0.15)',

    // Syntax highlighting - red monochrome
    syntax: {
      comment: '#661010',
      keyword: '#ff1a1a',
      string: '#cc3333',
      number: '#dd4400',
      function: '#ee1111',
      variable: '#cc1111',
      type: '#dd2222',
      operator: '#881010',
    },
  },
}
