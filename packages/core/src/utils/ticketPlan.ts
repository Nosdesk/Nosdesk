/**
 * A ticket's plan, read one way: which date supplies its plan day, the
 * time on that date, and its size. Pure functions over ticket fields.
 *
 * A time is a floating wall-clock time (`HH:MM:SS`), like the day it
 * belongs to: "2:30 pm" means the same to every viewer and is never
 * shifted through a zone. A size is stored in minutes and shown as a
 * letter; the presets write fixed minutes, and any other value still
 * reads as the nearest letter by range.
 *
 * `planPatchEffects` mirrors the backend's `services::ticket_plan` so an
 * optimistic write shows what the server will store.
 */
import { getDateConfig } from './dateUtils'
import { planDay, startDay, type TicketDates } from './ticketDates'

export interface TicketPlanFields extends TicketDates {
  due_time?: string | null
  start_time?: string | null
  rank?: string | null
  estimate_minutes?: number | null
  assignee_uuid?: string | null
}

// ---------------------------------------------------------------
// Time
// ---------------------------------------------------------------

export type PlanField = 'start' | 'due'

/** Which date supplies the plan day: the start date when set, else due. */
export function planField(ticket: TicketPlanFields): PlanField {
  return startDay(ticket) ? 'start' : 'due'
}

/** The time on the date that supplies the plan day, `HH:MM:SS` or null. */
export function planTime(ticket: TicketPlanFields): string | null {
  return (planField(ticket) === 'start' ? ticket.start_time : ticket.due_time) ?? null
}

/** The patch that sets the plan time, on whichever date supplies the day. */
export function planTimePatch(
  ticket: TicketPlanFields,
  time: string | null,
): { start_time: string | null } | { due_time: string | null } {
  return planField(ticket) === 'start' ? { start_time: time } : { due_time: time }
}

/** The patch that books a job on `day` at `time`. On its own plan day
 *  that is the time alone; from another day (a job carried in from an
 *  earlier one) it also moves the date that supplies the plan day, so
 *  the time lands on `day`. */
export function bookPatch(
  ticket: TicketPlanFields,
  day: string,
  time: string,
): TicketPlanFields {
  if (planDay(ticket) === day) return planTimePatch(ticket, time)
  const date = `${day}T00:00:00`
  return planField(ticket) === 'start' ? { start_date: date, start_time: time } : { due_date: date, due_time: time }
}

/** `HH:MM:SS` or `HH:MM` to minutes after midnight. */
export function toMinutes(time: string): number {
  const [h, m] = time.split(':').map(Number)
  return h * 60 + m
}

/** Minutes after midnight to `HH:MM`, clamped to the day's last quarter
 *  hour. */
export function toHhmm(minutes: number): string {
  const m = Math.max(0, Math.min(23 * 60 + 45, minutes))
  return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`
}

/** A floating wall-clock time in the active locale: "2:30 pm". */
export function formatWallTime(time: string, locale: string = getDateConfig().defaultLocale): string {
  const [h, m] = time.split(':').map(Number)
  return new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit', timeZone: 'UTC' })
    .format(new Date(Date.UTC(2000, 0, 1, h, m)))
}

// ---------------------------------------------------------------
// Size
// ---------------------------------------------------------------

export type Size = 's' | 'm' | 'l'

/** What each preset writes to `estimate_minutes`. */
export const SIZE_MINUTES: Record<Size, number> = { s: 30, m: 120, l: 360 }
export const SIZES: readonly Size[] = ['s', 'm', 'l']

/** Minutes back to a letter by range (up to an hour is small, up to four
 *  hours medium, more is big), so a value set through the API still
 *  shows one. */
export function sizeOf(minutes: number | null | undefined): Size | null {
  if (minutes == null) return null
  if (minutes <= 60) return 's'
  if (minutes <= 240) return 'm'
  return 'l'
}

/** Summed estimates of the sized jobs, in minutes. */
export function loadMinutes(tickets: readonly TicketPlanFields[]): number {
  return tickets.reduce((sum, t) => sum + (t.estimate_minutes ?? 0), 0)
}

/** Hours to the nearest half: 5, 8.5. */
export function loadHours(minutes: number): number {
  return Math.round(minutes / 30) / 2
}

// ---------------------------------------------------------------
// What a write brings with it
// ---------------------------------------------------------------

/**
 * The fields a plan write changes besides the ones it names, as the
 * server will store them: clearing a date clears its time, and a move to
 * another plan day or another assignee without a rank clears the rank.
 * Merge the result into the forward patch, and the current values of the
 * same fields into the inverse.
 */
export function planPatchEffects(
  current: TicketPlanFields,
  patch: TicketPlanFields,
): TicketPlanFields {
  const out: TicketPlanFields = {}
  if ('due_date' in patch && patch.due_date == null && !('due_time' in patch) && current.due_time != null) {
    out.due_time = null
  }
  if ('start_date' in patch && patch.start_date == null && !('start_time' in patch) && current.start_time != null) {
    out.start_time = null
  }
  if (!('rank' in patch) && current.rank != null) {
    const next = { ...current, ...patch }
    const movedDay = planDay(next) !== planDay(current)
    const reassigned = 'assignee_uuid' in patch && (patch.assignee_uuid ?? null) !== (current.assignee_uuid ?? null)
    if (movedDay || reassigned) out.rank = null
  }
  return out
}
