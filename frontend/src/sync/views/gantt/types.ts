/**
 * Shared gantt types, the one encoding used for date write-back, and
 * the label formatters.
 *
 * Structural on purpose: REST DTOs and pool rows both satisfy
 * `GanttCycle` without adaptation.
 *
 * The board reckons time in the browser's calendar: dates arrive as
 * naive `YYYY-MM-DDT00:00:00`, parse to local midnight, and go back the
 * same way. Labels therefore format in the local zone, not the user's
 * configured one, or a local midnight would print as the day before.
 */
import { getLocalTimeZone, toCalendarDateString } from '@nosdesk/core/utils/dateMath'
import { formatDate, formatDateRange, type DatePreset } from '@nosdesk/core/utils/dateUtils'

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
  return `${toCalendarDateString(d)}T00:00:00`
}

/** A board date in the active locale, e.g. "Sep 17" / "17 Sep". */
export function dayLabel(d: Date, preset: DatePreset = 'monthDay'): string {
  return formatDate(d, preset, getLocalTimeZone())
}

/** Range with shared parts collapsed: "Sep 7 – 13" / "7–13 Sep". */
export function dayRangeLabel(from: Date, to: Date): string {
  return formatDateRange(from, to, 'monthDay', getLocalTimeZone())
}
