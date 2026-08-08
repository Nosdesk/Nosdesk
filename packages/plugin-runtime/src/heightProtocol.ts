/**
 * The guest -> host height protocol, as a pure decision.
 *
 * Extracted from `observeHeight` so the rules can be asserted directly. They
 * are subtle enough to have produced two bugs already: a `requestAnimationFrame`
 * chase that never ran because rendering is suspended in a hidden iframe, and a
 * `last` seed of `-1` that collided with the HAS_CONTENT sentinel and swallowed
 * the first report from a panel measured while hidden.
 *
 * Three states go over the wire:
 *   * `> 0` — the content height in px. The host pins the iframe to it.
 *   * `0`   — the plugin rendered nothing. The host collapses the whole
 *             contribution, chrome included.
 *   * `-1`  — the plugin HAS content but cannot be measured right now, because
 *             the host has hidden it after a previous empty report and hiding
 *             suspends layout. The host restores layout; the guest then chases
 *             the real height (mutations still fire while hidden, which is how
 *             this is noticed at all).
 */

export const HAS_CONTENT_UNMEASURED = -1

export interface HeightInput {
  /** The plugin's root drew nothing: no element children, no text. Measured
   *  from CONTENT, never from height — a root that measures 0 only because the
   *  host collapsed the frame must NOT read as empty, or the two latch each
   *  other at zero and it can never grow back. */
  isEmpty: boolean
  /** Measured content height. 0 when layout is suspended (host hid us). */
  measuredPx: number
  /** Last value reported, or null if nothing has been reported yet. Null
   *  rather than a number so no seed can collide with a sentinel. */
  last: number | null
}

export interface HeightDecision {
  /** Value to post to the host, or null to stay quiet (deduped). */
  report: number | null
  /** The `last` value to carry forward. */
  last: number | null
  /** Start re-measuring: we have content but could not size it, so the host
   *  needs to give layout back before the real height can be read. */
  chase: boolean
}

export function decideHeightReport(input: HeightInput): HeightDecision {
  const { isEmpty, measuredPx, last } = input

  if (isEmpty) {
    // Deduped: only announce the transition into empty.
    if (last === 0) return { report: null, last, chase: false }
    return { report: 0, last: 0, chase: false }
  }

  if (measuredPx > 0) {
    if (measuredPx === last) return { report: null, last, chase: false }
    return { report: measuredPx, last: measuredPx, chase: false }
  }

  // Non-empty but unmeasurable: ask for layout back, then chase the real value.
  if (last === HAS_CONTENT_UNMEASURED) return { report: null, last, chase: false }
  return { report: HAS_CONTENT_UNMEASURED, last: HAS_CONTENT_UNMEASURED, chase: true }
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
