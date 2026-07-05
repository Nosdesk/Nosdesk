/**
 * Cycle start/end dates are calendar days the user picks, stored as
 * instants. One policy for both directions keeps the picked day
 * stable across timezones:
 *
 * - write: midday UTC (`T12:00:00Z`), so every real-world offset
 *   (UTC-12 to UTC+14) renders the same calendar day. The old code
 *   round-tripped the picker's `yyyy-MM-dd` through `new Date()`,
 *   whose parsing is offset-sensitive and could shift a day.
 * - read: the ISO string's UTC date part, which under this policy
 *   (and for legacy midnight-UTC rows) is the picked day.
 *
 * Used by every cycle create/edit path; do not hand-roll date
 * conversion at a call site.
 */

/** ISO instant -> `yyyy-MM-dd` for a DatePicker, or '' when unset. */
export function isoToDateInput(iso: string | null | undefined): string {
  return iso ? iso.slice(0, 10) : ''
}

/** DatePicker `yyyy-MM-dd` -> ISO instant, or null when cleared. */
export function dateInputToIso(day: string): string | null {
  return day ? `${day}T12:00:00.000Z` : null
}
