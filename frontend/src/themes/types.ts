/**
 * Theme System Type Definitions
 *
 * Defines the structure for theme colors, metadata, and preferences.
 * All themes must implement these interfaces to ensure consistency.
 */

/**
 * Core semantic color tokens that all themes must define
 */
export interface ThemeColors {
  // Background layers (most to least prominent)
  app: string
  surface: string
  surfaceAlt: string
  surfaceHover: string

  // Border colors (subtle to strong)
  default: string
  subtle: string
  strong: string

  // Text hierarchy
  primary: string
  secondary: string
  tertiary: string

  // Accent colors
  accent: string
  accentHover: string
  accentMuted: string

  // Status colors
  success: string
  successMuted: string
  error: string
  errorMuted: string
  warning: string
  warningMuted: string
  info: string
  infoMuted: string

  // Ticket/workflow status colors
  statusOpen: string
  statusOpenMuted: string
  statusInProgress: string
  statusInProgressMuted: string
  statusClosed: string
  statusClosedMuted: string

  // Priority colors
  priorityHigh: string
  priorityHighMuted: string
  priorityMedium: string
  priorityMediumMuted: string
  priorityLow: string
  priorityLowMuted: string

  // Shadows
  shadowDark: string
  shadowLight: string

  // Syntax/semantic colors (for editors, code blocks) - optional
  syntax?: {
    comment: string
    keyword: string
    string: string
    number: string
    function: string
    variable: string
    type: string
    operator: string
  }
}

/**
 * Theme metadata
 */
export interface ThemeMeta {
  id: string
  name: string
  author?: string
  version?: string
  description?: string
  isDark: boolean
  category: 'builtin' | 'community' | 'custom'
}

/**
 * Complete theme definition
 */
export interface Theme {
  meta: ThemeMeta
  colors: ThemeColors
}

/**
 * User's theme preference stored in backend
 */
export interface UserThemePreference {
  theme: 'system' | string
  accentColor?: string | null
}

/**
 * Built-in theme IDs
 */
export type BuiltInThemeId =
  | 'system'
  | 'light'
  | 'dark'
  | 'nord'
  | 'tokyo-night'
  | 'gruvbox-light'
  | 'gruvbox-dark'
  | 'dracula'
  | 'everforest-light'
  | 'everforest-dark'
  | 'catppuccin-latte'
  | 'catppuccin-mocha'
  | 'one-dark'
  | 'epaper'
  | 'christmas'

/**
 * Theme mode - either 'system' or a specific theme ID
 */
export type ThemeMode = 'system' | string
