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

/**
 * Compact-dot rendering of a tone, for chrome too tight for a full
 * pill (e.g. the project card's cycle glance). One mapping so a dot
 * and a pill driven by the same tone can never disagree.
 */
export function toneDotClass(tone: StatusPillTone | undefined): string {
  switch (tone) {
    case 'positive':
      return 'bg-status-success'
    case 'caution':
      return 'bg-status-warning'
    case 'critical':
      return 'bg-status-error'
    case 'info':
      return 'bg-status-info'
    case 'accent':
      return 'bg-accent'
    default:
      return 'bg-strong'
  }
}
