import type { Theme } from '../types'

/**
 * Christmas Theme
 *
 * A festive holiday theme with traditional Christmas colors.
 * Only available during December.
 *
 * Color palette:
 * - #FC4242 - Bright red
 * - #D41616 - Dark red
 * - #318507 - Green
 * - #0C5E15 - Dark green
 * - #173404 - Very dark green (backgrounds)
 */
export const christmasTheme: Theme = {
  meta: {
    id: 'christmas',
    name: 'Christmas',
    author: 'Nosdesk',
    description: 'A festive holiday theme for the Christmas season',
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Deep forest green backgrounds
    app: '#0d1a08',
    surface: '#142210',
    surfaceAlt: '#1c2e16',
    surfaceHover: '#243a1c',

    // Borders with green tint
    default: '#2d4a22',
    subtle: '#1c2e16',
    strong: '#3d6030',

    // Snow white text hierarchy
    primary: '#fff8f0',
    secondary: '#e8e4dc',
    tertiary: '#b8b4ac',

    // Bright red accent
    accent: '#FC4242',
    accentHover: '#ff6060',
    accentMuted: 'rgba(252, 66, 66, 0.2)',

    // Status colors with Christmas palette
    success: '#318507', // Christmas green
    successMuted: 'rgba(49, 133, 7, 0.2)',
    error: '#D41616', // Dark red
    errorMuted: 'rgba(212, 22, 22, 0.2)',
    warning: '#FC4242', // Bright red
    warningMuted: 'rgba(252, 66, 66, 0.2)',
    info: '#5a9a6a', // Softer green for info
    infoMuted: 'rgba(90, 154, 106, 0.2)',

    // Ticket status - festive colors
    statusOpen: '#FC4242', // Bright red for open
    statusOpenMuted: 'rgba(252, 66, 66, 0.2)',
    statusInProgress: '#5a9a6a', // Soft green
    statusInProgressMuted: 'rgba(90, 154, 106, 0.2)',
    statusClosed: '#318507', // Christmas green
    statusClosedMuted: 'rgba(49, 133, 7, 0.2)',

    // Priority with Christmas colors
    priorityHigh: '#D41616', // Dark red
    priorityHighMuted: 'rgba(212, 22, 22, 0.2)',
    priorityMedium: '#FC4242', // Bright red
    priorityMediumMuted: 'rgba(252, 66, 66, 0.2)',
    priorityLow: '#318507', // Christmas green
    priorityLowMuted: 'rgba(49, 133, 7, 0.2)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.4)',
    shadowLight: 'rgba(255, 248, 240, 0.05)',

    // Syntax highlighting with festive colors
    syntax: {
      comment: '#5a7050',
      keyword: '#D41616',
      string: '#318507',
      number: '#FC4242',
      function: '#5a9a6a',
      variable: '#e8e4dc',
      type: '#0C5E15',
      operator: '#D41616',
    },
  },
}
