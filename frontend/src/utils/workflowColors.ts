/**
 * Maps a workflow state's design-token color name to the existing
 * Tailwind status-color classes. Workflow states ship with the seed
 * names (`slate`, `gray`, `blue`, `purple`, `green`, `subtle`) but
 * admins will eventually pick from the same palette when adding new
 * states; centralising the mapping keeps badge rendering consistent
 * with the legacy status-bucket palette.
 */

export interface BadgePaletteClasses {
  /** Background, text, and border classes for the badge body. */
  badge: string
  /** Solid color class suitable for icons / dots. */
  solid: string
}

const PALETTE: Record<string, BadgePaletteClasses> = {
  slate: {
    badge: 'bg-status-open-muted text-status-open border border-status-open/30',
    solid: 'text-status-open',
  },
  gray: {
    badge: 'bg-status-open-muted text-status-open border border-status-open/30',
    solid: 'text-status-open',
  },
  blue: {
    badge:
      'bg-status-in-progress-muted text-status-in-progress border border-status-in-progress/30',
    solid: 'text-status-in-progress',
  },
  purple: {
    badge:
      'bg-status-in-progress-muted text-status-in-progress border border-status-in-progress/30',
    solid: 'text-status-in-progress',
  },
  green: {
    badge: 'bg-status-closed-muted text-status-closed border border-status-closed/30',
    solid: 'text-status-closed',
  },
  subtle: {
    badge: 'bg-surface-alt text-secondary border border-default',
    solid: 'text-secondary',
  },
}

const FALLBACK: BadgePaletteClasses = {
  badge: 'bg-surface-alt text-secondary border border-default',
  solid: 'text-secondary',
}

export function paletteForColor(color: string | null | undefined): BadgePaletteClasses {
  if (!color) return FALLBACK
  return PALETTE[color] ?? FALLBACK
}
