/**
 * Gantt geometry constants. One module so the lane column and the
 * timeline never disagree about row math: every `y` in the board is
 * derived from these numbers by `rowModel.ts`, and no renderer is
 * allowed to multiply row indexes by a literal.
 */

/** Row height (px). Comfortable default; the density toggle maps to
 * the other steps. */
export const ROW_PX = 40

/** Row height per density step (the shared ListDensityToggle). */
export const DENSITY_ROW_PX: Record<'compact' | 'cosy' | 'comfortable', number> = {
  compact: 28,
  cosy: 34,
  comfortable: ROW_PX,
}

/** Vertical inset of a bar inside its row (px): bar top = row y +
 * inset, bar height = row height - 2 * inset. */
export const BAR_INSET_Y = 4

/** Group header row height (px), shorter than a card row. */
export const GROUP_ROW_PX = 32

/** Minimum interactive bar width (px). At quarter zoom a 1-day bar
 * would otherwise be a ~3px sliver nobody can hover or click. */
export const MIN_BAR_PX = 12

/** Below this bar width (px) the title renders outside the bar, to
 * its right, instead of truncating inside. */
export const LABEL_MIN_PX = 48

/** Bar width (px) needed before the in-bar assignee avatar shows. */
export const AVATAR_MIN_PX = 72

/** Motion durations (ms), shared by the board's transitions. All
 * usages gate on motion-safe / useReducedMotion. */
export const MOTION_FAST_MS = 120
export const MOTION_SETTLE_MS = 150
