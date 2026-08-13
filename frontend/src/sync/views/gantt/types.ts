/**
 * Shared gantt types and the one encoding used for date write-back.
 *
 * Structural on purpose: REST DTOs and pool rows both satisfy
 * `GanttCycle` without adaptation.
 */
import { format } from 'date-fns'

/** The slice of a cycle the board renders (bands + grouping labels). */
export interface GanttCycle {
  id: number
  uuid: string
  name: string
  state: 'planned' | 'active' | 'completed'
  start_at?: string | null
  end_at?: string | null
}

/**
 * Naive local-midnight datetime (no tz suffix). Dates round-trip
 * through the backend's NaiveDateTime model, whose deserialiser
 * rejects a trailing `Z`; sending the local day also keeps the bar
 * anchored to the day the user dropped it on.
 */
export function naiveDay(d: Date): string {
  return `${format(d, 'yyyy-MM-dd')}T00:00:00`
}
