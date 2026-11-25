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

/** Settings > Appearance > Text size's stops, in percent. 200 is there so a
 *  person's own size can reach as far as the device's: without it, turning
 *  "Use device text size" off at the largest sizes would drop the text. */
export const TEXT_SIZE_STOPS = [100, 112, 125, 150, 175, 200] as const

/** The scale in force: the device's while following it, the person's own
 *  stop otherwise, held to 1 to 2 as `--text-scale` holds it. */
export function composeScale(follow: boolean, deviceScale: number, ownPercent: number): number {
  return clampScale(follow ? deviceScale : ownPercent / 100)
}

/** The index of the stop nearest a percentage, for placing the slider's
 *  thumb where the device has put the text. */
export function nearestStop(percent: number): number {
  let best = 0
  for (let i = 1; i < TEXT_SIZE_STOPS.length; i++) {
    if (Math.abs(TEXT_SIZE_STOPS[i] - percent) < Math.abs(TEXT_SIZE_STOPS[best] - percent)) best = i
  }
  return best
}

/** The smallest an email's body text may show while it is shrunk to fit, in px. */
export const EMAIL_TEXT_FLOOR_PX = 12

/**
 * The zoom that fits an email `overflow` times wider than its frame at `zoom`,
 * held where its body text (`bodyPx` at zoom 1) shows at 12px; past that the
 * email pans. Small print goes under 12px with it; body text the sender
 * already set under 12px at the reader's size is not shrunk further.
 */
export function fitEmailZoom(zoom: number, overflow: number, bodyPx: number): number {
  const floor = Math.min(zoom, EMAIL_TEXT_FLOOR_PX / Math.max(1, bodyPx))
  return Math.max(floor, zoom / Math.max(1, overflow))
}
