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
  /** Self-contained "paint this element a solid colour" class
   * string. Used on empty `<span class="rounded-full">` status
   * dots; the `text-*` token sets the colour and `bg-current`
   * paints the background to match (otherwise the dot has no
   * content for a text-colour to colour, and renders invisible).
   *
   * The earlier API exposed only the `text-*` token, requiring
   * every caller to remember to also add `bg-current`. Most
   * consumers forgot, producing invisible dots in TicketsTable
   * + TicketsCardList — the bug surfaced via the table's
   * Backlog status column. Bundling the pair into one string
   * is the foot-gun-free shape; consumers that already paired
   * with `bg-current` (CustomDropdown) just emit a harmless
   * duplicate. */
  solid: string
}

/**
 * The six colour tokens the seed workflow ships (slate for Triage, gray,
 * blue, purple for In Review, green, subtle), each with its own palette:
 * the open / in-progress / closed status styles for gray, blue and green,
 * a neutral "subtle" for cancelled, and slate and violet for the two that
 * would otherwise repeat a status colour. [`SUPPORTED_COLOR_TOKENS`] lists
 * them for the colour picker.
 */
const PALETTE: Record<string, BadgePaletteClasses> = {
  // Intake that hasn't been looked at yet: cool neutral.
  slate: {
    badge: 'bg-slate-500/15 text-slate-700 dark:text-slate-300 border border-slate-500/30',
    solid: 'text-slate-500 bg-current',
  },
  // Open-bucket palette: low-effort intake.
  gray: {
    badge: 'bg-status-open-muted text-status-open border border-status-open/30',
    solid: 'text-status-open bg-current',
  },
  // In-progress palette: active work.
  blue: {
    badge:
      'bg-status-in-progress-muted text-status-in-progress border border-status-in-progress/30',
    solid: 'text-status-in-progress bg-current',
  },
  // Waiting on review: active work, set apart from blue.
  purple: {
    badge: 'bg-violet-500/15 text-violet-700 dark:text-violet-300 border border-violet-500/30',
    solid: 'text-violet-500 bg-current',
  },
  // Closed palette: terminal completion.
  green: {
    badge: 'bg-status-closed-muted text-status-closed border border-status-closed/30',
    solid: 'text-status-closed bg-current',
  },
  // Neutral palette: cancelled / archived.
  subtle: {
    badge: 'bg-surface-alt text-secondary border border-default',
    solid: 'text-secondary bg-current',
  },
}

const FALLBACK: BadgePaletteClasses = {
  badge: 'bg-surface-alt text-secondary border border-default',
  solid: 'text-secondary bg-current',
}

/**
 * Color tokens the admin UI exposes in the workflow-state picker: every
 * token with a palette, so a seeded state's colour is always one it can
 * show as selected.
 */
export const SUPPORTED_COLOR_TOKENS = ['slate', 'gray', 'blue', 'purple', 'green', 'subtle'] as const

export function paletteForColor(color: string | null | undefined): BadgePaletteClasses {
  if (!color) return FALLBACK
  return PALETTE[color] ?? FALLBACK
}
