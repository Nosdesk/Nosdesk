/**
 * The order tickets take when sorted by Due: the due day, then the due
 * time (untimed jobs after timed ones), then the hand-placed rank, then
 * priority with the most urgent first, then the oldest created first.
 * Tickets with no due date go last in both directions.
 *
 * Descending reverses the day and the time only. The keys below it
 * break ties the same way either way: within one day the most urgent job
 * still comes first.
 */
import type { CardData } from './types'
import { priorityRank } from '../../constants/ticketOptions'
import { parseDate } from '../../utils/dateUtils'
import { dueDay, type Day } from '../../utils/ticketDates'

export interface DueOrderKey {
  day: Day | null
  /** Due time of day, `HH:MM:SS`. */
  time: string | null
  /** Fractional index from a hand placement (`planOrder.ts`). */
  rank: string | null
  priority: number
  created: number
}

export function dueOrderKey(card: CardData): DueOrderKey {
  return {
    day: dueDay(card),
    time: card.due_time ?? null,
    rank: card.rank ?? null,
    priority: priorityRank(card.priority),
    created: parseDate(card.created_at)?.getTime() ?? 0,
  }
}

/** Nulls after values; otherwise plain string order. */
function compareNullsLast(a: string | null, b: string | null): number {
  if (a === b) return 0
  if (a == null) return 1
  if (b == null) return -1
  return a < b ? -1 : 1
}

export function compareDueOrder(
  a: DueOrderKey,
  b: DueOrderKey,
  dir: 'asc' | 'desc' = 'asc',
): number {
  const sign = dir === 'asc' ? 1 : -1
  if (a.day !== b.day) {
    if (a.day == null) return 1
    if (b.day == null) return -1
    return (a.day < b.day ? -1 : 1) * sign
  }
  if (a.time !== b.time) {
    if (a.time == null) return 1
    if (b.time == null) return -1
    return (a.time < b.time ? -1 : 1) * sign
  }
  const rank = compareNullsLast(a.rank, b.rank)
  if (rank !== 0) return rank
  if (a.priority !== b.priority) return b.priority - a.priority
  return a.created - b.created
}
