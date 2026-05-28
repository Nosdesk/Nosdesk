/**
 * SLA payload shapes — mirrors `services::sla::{SlaTimer, SlaPill}` on
 * the backend.
 *
 * The backend flattens the primary (most-urgent) timer's fields onto
 * the top level of the JSON payload so v1 consumers that read
 * `sla.breached` / `sla.paused` / `sla.target_at` keep working
 * unchanged — they now reflect whichever timer is currently most at
 * risk. The nested `response` + `resolution` sub-objects are additive:
 * the preview pane uses them to stack both timers; the list pill and
 * filter facets continue to read the flat fields.
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
