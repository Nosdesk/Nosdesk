/**
 * Vertical-timeline geometry, as pure functions.
 *
 * Extracted from the component so the load-bearing claim of the design can be
 * asserted rather than eyeballed: a horizontal gantt is bounded by TIME SPAN,
 * the vertical one by CONCURRENCY, and the whole approach only holds if
 * concurrency degrades gracefully. That is a property of these two functions,
 * so they are tested directly at 2, 4, 8 and 15 parallel tickets instead of
 * being inferred from a screenshot of whatever the demo data happens to hold.
 */

/** Gutter holding the time ruler. */
export const GUTTER = 48
/** Fidelity thresholds, in px of column width. */
export const WIDTH_FULL = 132
export const WIDTH_COMPACT = 68
/** Minimum block height that can carry a title without clipping it. */
export const HEIGHT_TITLE = 56
/**
 * Legibility floor for a column.
 *
 * Set just under the width four lanes get on a 390px phone, so four still share
 * the viewport exactly and the floor only binds from five. Dividing by concurrency
 * alone put five parallel tickets at 67px and dropped the ENTIRE view to
 * id-only marks, and five in flight is an ordinary week rather than an edge
 * case. Past this point concurrency scrolls sideways instead of shrinking.
 *
 * The two-axis scroll this introduces is what the horizontal gantt was rejected
 * for, but the shape is inverted: there the unbounded axis (90 days, 3737px)
 * was the one you had to pan; here panning is bounded by concurrency, capped by
 * how much work can genuinely be in flight, and does not happen at all below
 * five.
 */
export const MIN_LANE_PX = 80

export interface Span {
  start: Date
  end: Date
}

/**
 * Greedy interval partitioning: each item takes the first column free at its
 * start. The resulting column count is exactly peak concurrency, which is the
 * quantity this layout is bounded by.
 */
export function assignLanes<T extends Span>(items: readonly T[]): Array<{ item: T; lane: number }> {
  const sorted = [...items].sort((a, b) => a.start.getTime() - b.start.getTime())
  const laneFreeAt: number[] = []
  const out: Array<{ item: T; lane: number }> = []
  for (const item of sorted) {
    let lane = laneFreeAt.findIndex((free) => free <= item.start.getTime())
    if (lane === -1) {
      lane = laneFreeAt.length
      laneFreeAt.push(0)
    }
    laneFreeAt[lane] = item.end.getTime()
    out.push({ item, lane })
  }
  return out
}

/** Peak concurrency, i.e. how many columns the layout needs. */
export function laneCount(placed: ReadonlyArray<{ lane: number }>): number {
  return Math.max(1, new Set(placed.map((p) => p.lane)).size)
}

/**
 * Width each column gets on a viewport of `viewportPx`.
 *
 * Columns share the viewport until sharing would take them below the legibility
 * floor, after which they hold that width and the canvas grows past the
 * viewport. Callers must size the canvas with `canvasWidth`, not the viewport.
 */
export function laneWidth(viewportPx: number, lanes: number): number {
  return Math.max(MIN_LANE_PX, (viewportPx - GUTTER - 8) / Math.max(1, lanes))
}

/** Total canvas width. Exceeds the viewport exactly when concurrency has pushed
 *  columns onto the legibility floor, which is what makes it pan sideways. */
export function canvasWidth(viewportPx: number, lanes: number): number {
  return Math.max(viewportPx, GUTTER + lanes * laneWidth(viewportPx, lanes) + 8)
}

/**
 * mark -> chip -> card, by how much room a block actually got.
 *
 * Keyed on BOTH axes. A one-day deadline marker is ~36px tall however wide its
 * column is, and a title rendered into it clips mid-word.
 */
export function fidelityFor(widthPx: number, heightPx: number): 'full' | 'compact' | 'mark' {
  if (heightPx < HEIGHT_TITLE) return 'mark'
  if (widthPx >= WIDTH_FULL) return 'full'
  if (widthPx >= WIDTH_COMPACT) return 'compact'
  return 'mark'
}
