/**
 * The guest -> host height protocol, as a pure decision.
 *
 * Two states go over the wire:
 *   * `> 0` — the content height in px. The host pins the iframe to it.
 *   * `0`   — the plugin rendered nothing. The host drops its chrome and
 *             collapses the contribution to zero height.
 *
 * There is deliberately no "has content but cannot be measured" sentinel. An
 * earlier version collapsed with `display: none`, which suspends layout inside
 * the iframe, so a plugin that filled in later could no longer measure itself
 * and needed a third state to ask for layout back, plus a timer loop to chase
 * the real height once it returned. That chain existed only to avoid one stray
 * flex gap, and it produced two bugs of its own. Collapsing with `block-size: 0`
 * keeps layout alive, so the guest's ResizeObserver keeps working and a plugin
 * that fills in late is reported the same way as any other content change.
 *
 * Emptiness is measured from CONTENT, never from height: a root that measures 0
 * because the host collapsed the frame must not read as empty, or the two latch
 * each other at zero and it can never grow back.
 */

export interface HeightInput {
  /** The plugin's root drew nothing: no element children, no text. */
  isEmpty: boolean
  /** Measured content height. */
  measuredPx: number
  /** Last value reported, or null if nothing has been reported yet. */
  last: number | null
}

export interface HeightDecision {
  /** Value to post to the host, or null to stay quiet (deduped). */
  report: number | null
  /** The `last` value to carry forward. */
  last: number | null
}

export function decideHeightReport(input: HeightInput): HeightDecision {
  const { isEmpty, measuredPx, last } = input
  // Clamp: a measurement can only ever be a size, never negative.
  const height = isEmpty ? 0 : Math.max(0, measuredPx)
  if (height === last) return { report: null, last }
  return { report: height, last: height }
}

/** Container-width buckets for `data-nd-container`.
 *
 *  Deliberately NOT the app's breakpoints: a panel lives in roughly 300-700px,
 *  where `sm`/`md`/`lg` would put nearly everything in one bucket and tell a
 *  plugin nothing useful about its own width. */
export const CONTAINER_NARROW_MAX = 480
export const CONTAINER_MEDIUM_MAX = 768

export function containerSize(width: number): 'narrow' | 'medium' | 'wide' {
  if (width < CONTAINER_NARROW_MAX) return 'narrow'
  if (width < CONTAINER_MEDIUM_MAX) return 'medium'
  return 'wide'
}
