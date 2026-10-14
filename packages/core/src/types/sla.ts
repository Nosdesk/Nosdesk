/**
 * SLA payload shapes — mirrors `services::sla::{SlaTimer, SlaPill}` on
 * the backend.
 *
 * The backend flattens one timer onto the top level of the payload,
 * with its state and its countdown kept apart:
 * - `breached` (and `pill_color` red) is the ticket's: true when any
 *   timer breached, computed or stamped by the breach job, and wasn't
 *   met in time. It agrees with every breach notification.
 * - `target_at`, `start_at`, `met_at`, `paused` and `seconds_remaining`
 *   belong to the timer to count down to: the unmet timer due first,
 *   else the earliest breached timer, else whichever exists.
 * So after a late response the pill is red and counts down to the
 * resolution target. The nested `response` + `resolution` sub-objects
 * carry each timer as it is, for the preview pane to stack.
 */

export interface SlaTimer {
  /** Wall-clock start of the timer (ticket's `created_at` today).
   *  Lets the frontend derive the at-risk threshold live (within 25%
   *  of `target_at - start_at` remaining flips amber). */
  start_at: string
  target_at: string
  /** ISO timestamp when the timer was satisfied (e.g.
   * `first_response_at` for the response timer). Omitted when the
   * timer is still ticking. */
  met_at?: string | null
  breached: boolean
  paused: boolean
  pill_color: 'green' | 'amber' | 'red'
  seconds_remaining?: number | null
}

export interface SlaPill extends SlaTimer {
  /** Response-target timer, present when the matched policy has
   * `target_response_minutes` configured. */
  response?: SlaTimer
  /** Resolution-target timer, present when the matched policy has
   * `target_resolution_minutes` configured. */
  resolution?: SlaTimer
}
