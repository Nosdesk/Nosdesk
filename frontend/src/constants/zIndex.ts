/**
 * Centralized z-index scale.
 *
 * Tailwind classes (z-header, z-backdrop, z-overlay, z-effect, z-cursor)
 * are defined in tailwind.config.js and should be used in templates.
 *
 * These constants are for use in JavaScript/TypeScript where inline
 * styles are required (e.g. composables that create DOM elements).
 */
export const Z_INDEX = {
  /** Site header bar */
  HEADER: 100,
  /** Modal / bottom-sheet backdrop overlays */
  BACKDROP: 200,
  /** Modals, global search, toasts, notifications, tooltips */
  OVERLAY: 300,
  /** Visual theme effects (CRT, snowfall) — pointer-events: none */
  EFFECT: 400,
  /** Cursor effects (scanlines) — pointer-events: none */
  CURSOR: 500,
} as const
