import type { Theme } from '../types'

/**
 * One Dark Theme
 *
 * Atom's iconic One Dark theme.
 * Based on: https://github.com/atom/atom/tree/master/packages/one-dark-syntax
 */
export const oneDarkTheme: Theme = {
  meta: {
    id: 'one-dark',
    name: 'One Dark',
    author: 'Atom',
    description: "Atom's iconic One Dark theme",
    isDark: true,
    category: 'builtin',
  },
  colors: {
    // Backgrounds
    app: '#282c34',
    surface: '#21252b',
    surfaceAlt: '#2c313a',
    surfaceHover: '#3a3f4b',

    // Borders
    default: '#3a3f4b',
    subtle: '#2c313a',
    strong: '#4b5263',

    // Text
    primary: '#abb2bf',
    secondary: '#9da5b4',
    tertiary: '#636d83',

    // Accent (One Dark blue)
    accent: '#61afef',
    accentHover: '#74b9f0',
    accentMuted: 'rgba(97, 175, 239, 0.2)',

    // Status
    success: '#98c379',
    successMuted: 'rgba(152, 195, 121, 0.2)',
    error: '#e06c75',
    errorMuted: 'rgba(224, 108, 117, 0.2)',
    warning: '#e5c07b',
    warningMuted: 'rgba(229, 192, 123, 0.2)',
    info: '#61afef',
    infoMuted: 'rgba(97, 175, 239, 0.2)',

    // Ticket status
    statusOpen: '#e5c07b',
    statusOpenMuted: 'rgba(229, 192, 123, 0.2)',
    statusInProgress: '#61afef',
    statusInProgressMuted: 'rgba(97, 175, 239, 0.2)',
    statusClosed: '#98c379',
    statusClosedMuted: 'rgba(152, 195, 121, 0.2)',

    // Priority
    priorityHigh: '#e06c75',
    priorityHighMuted: 'rgba(224, 108, 117, 0.2)',
    priorityMedium: '#e5c07b',
    priorityMediumMuted: 'rgba(229, 192, 123, 0.2)',
    priorityLow: '#98c379',
    priorityLowMuted: 'rgba(152, 195, 121, 0.2)',

    // Shadows
    shadowDark: 'rgba(0, 0, 0, 0.4)',
    shadowLight: 'rgba(255, 255, 255, 0.03)',

    // Syntax highlighting
    syntax: {
      comment: '#5c6370',
      keyword: '#c678dd',
      string: '#98c379',
      number: '#d19a66',
      function: '#61afef',
      variable: '#e06c75',
      type: '#56b6c2',
      operator: '#abb2bf',
    },
  },
}
