import type { Theme } from '../types'

/**
 * E-Paper Theme
 *
 * High contrast 16-tone greyscale theme optimized for e-paper displays.
 * Features pure black borders and maximum contrast for readability.
 */
export const ePaperTheme: Theme = {
  meta: {
    id: 'epaper',
    name: 'E-Paper',
    description: 'High contrast greyscale theme for e-paper displays',
    isDark: false,
    category: 'builtin',
  },
  colors: {
    // Backgrounds - pure white to light grey
    app: '#eeeeee',
    surface: '#ffffff',
    surfaceAlt: '#dddddd',
    surfaceHover: '#cccccc',

    // Borders - black edges for maximum contrast
    default: '#000000',
    subtle: '#333333',
    strong: '#000000',

    // Text - pure black for maximum readability
    primary: '#000000',
    secondary: '#222222',
    tertiary: '#444444',

    // Accent - dark grey (no color on e-paper)
    accent: '#000000',
    accentHover: '#222222',
    accentMuted: '#cccccc',

    // Status - greyscale representations
    success: '#000000',
    successMuted: '#dddddd',
    error: '#000000',
    errorMuted: '#bbbbbb',
    warning: '#333333',
    warningMuted: '#cccccc',
    info: '#222222',
    infoMuted: '#dddddd',

    // Ticket status - differentiated by pattern/intensity
    statusOpen: '#555555',
    statusOpenMuted: '#dddddd',
    statusInProgress: '#333333',
    statusInProgressMuted: '#cccccc',
    statusClosed: '#000000',
    statusClosedMuted: '#eeeeee',

    // Priority - darker = higher priority
    priorityHigh: '#000000',
    priorityHighMuted: '#cccccc',
    priorityMedium: '#444444',
    priorityMediumMuted: '#dddddd',
    priorityLow: '#777777',
    priorityLowMuted: '#eeeeee',

    // Shadows - sharp black shadows
    shadowDark: 'rgba(0, 0, 0, 0.3)',
    shadowLight: 'rgba(255, 255, 255, 0.9)',

    // Syntax highlighting - greyscale differentiation
    syntax: {
      comment: '#666666',
      keyword: '#000000',
      string: '#333333',
      number: '#444444',
      function: '#111111',
      variable: '#222222',
      type: '#000000',
      operator: '#555555',
    },
  },
}
