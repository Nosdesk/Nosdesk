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
 * The seed workflow ships six color tokens (slate, gray, blue,
 * purple, green, subtle) but currently maps them to three distinct
 * visual palettes — the legacy open / in-progress / closed status
 * styles, plus a neutral "subtle" for cancelled. An admin who picks
 * `purple` and `blue` will see them render identically until the
 * design system grows distinct CSS variables per token. The
 * [`SUPPORTED_COLOR_TOKENS`] export is the SOT for color pickers, so
 * the admin UI can't promise more than rendering delivers.
 */
const PALETTE: Record<string, BadgePaletteClasses> = {
  // Open-bucket palette: low-effort intake.
  slate: {
    badge: 'bg-status-open-muted text-status-open border border-status-open/30',
    solid: 'text-status-open bg-current',
  },
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
  purple: {
    badge:
      'bg-status-in-progress-muted text-status-in-progress border border-status-in-progress/30',
    solid: 'text-status-in-progress bg-current',
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
 * Color tokens the admin UI exposes in the workflow-state picker.
 * Until the design system grows distinct CSS variables per token,
 * this list is intentionally narrowed to the visually-distinct
 * palettes — picking `slate` vs `gray` would otherwise look
 * identical in the kanban / status badge.
 */
export const SUPPORTED_COLOR_TOKENS = ['gray', 'blue', 'green', 'subtle'] as const

export function paletteForColor(color: string | null | undefined): BadgePaletteClasses {
  if (!color) return FALLBACK
  return PALETTE[color] ?? FALLBACK
}
