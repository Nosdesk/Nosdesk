/**
 * Tone vocabulary for <StatusPill>. Extracted to its own module so
 * non-component code (e.g. cycle-health classification) can reference the
 * tone type without importing the .vue file.
 */
export type StatusPillTone =
  | 'positive'
  | 'caution'
  | 'critical'
  | 'info'
  | 'accent'
  | 'neutral'
