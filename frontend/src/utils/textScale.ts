/**
 * Text size, as numbers. `--text-scale` (main.css) is held to 1 to 2: a
 * device set smaller than its default keeps the designed size, and 200% is
 * WCAG 1.4.4's mark (pinch zoom covers the rest).
 */
export const MIN_SCALE = 1
export const MAX_SCALE = 2

export function clampScale(value: number): number {
  if (!Number.isFinite(value)) return MIN_SCALE
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, value))
}

/** Reading text's line height, as a ratio of its size. */
const READING_LEADING = 1.5
/** Reading text at scale 1, in px (`--text-base`). */
const READING_SIZE = 16

/**
 * How many lines the reply field grows to before it scrolls: five, or as many
 * whole lines as fit in 150px once five would be taller, and never under two.
 * At 200% that is three, so the thread keeps some of the screen above the
 * keyboard.
 */
export function replyFieldLines(scale: number): number {
  const line = READING_SIZE * READING_LEADING * clampScale(scale)
  return Math.max(2, Math.min(5, Math.floor(150 / line)))
}
