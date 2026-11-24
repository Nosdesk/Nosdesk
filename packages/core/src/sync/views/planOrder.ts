/**
 * The order of a day's work, and the rank keys that hold a hand-placed
 * order.
 *
 * Inside one day:
 *   1. jobs with a plan time, by time (Booked);
 *   2. the rest (Any time): jobs placed by hand, by rank;
 *   3. then jobs never placed, by priority (most urgent first), then the
 *      oldest first.
 *
 * So an untouched day reads in priority order, and once someone drags,
 * their order wins for what they placed. The list's Manual sort is the
 * same order without the day.
 *
 * A rank is a fractional index key over ASCII digits and letters,
 * compared byte by byte (the column is `COLLATE "C"`), so a key can
 * always be made between two others and moving one job writes one
 * ticket. The backend checks the alphabet and length
 * (`services::ticket_plan::check_rank`).
 */
import type { CardData } from './types'
import { priorityRank } from '../../constants/ticketOptions'
import { parseDate } from '../../utils/dateUtils'
import { dueDay, planDay, type Day } from '../../utils/ticketDates'
import { planTime, toHhmm, toMinutes } from '../../utils/ticketPlan'

/** The sort field of the hand-placed order. */
export const MANUAL_SORT = 'rank'

function created(card: CardData): number {
  return parseDate(card.created_at)?.getTime() ?? 0
}

/** Jobs with a time, by time. */
export function compareBooked(a: CardData, b: CardData): number {
  const ta = planTime(a) ?? ''
  const tb = planTime(b) ?? ''
  if (ta !== tb) return ta < tb ? -1 : 1
  return a.id - b.id
}

/** Jobs without a time: placed ones by rank, then the rest by priority
 *  (most urgent first), then the oldest. */
export function compareAnyTime(a: CardData, b: CardData): number {
  const ra = a.rank ?? null
  const rb = b.rank ?? null
  if (ra !== rb) {
    if (ra == null) return 1
    if (rb == null) return -1
    return ra < rb ? -1 : 1
  }
  if (ra != null) return a.id - b.id
  const pa = priorityRank(a.priority)
  const pb = priorityRank(b.priority)
  if (pa !== pb) return pb - pa
  return created(a) - created(b)
}

/** The list's Manual sort: placed jobs by rank, then the rest by
 *  priority and age. Rank is compared across whatever is on screen. */
export const compareManual = compareAnyTime

export interface DayPlan {
  booked: CardData[]
  anyTime: CardData[]
}

/** The jobs planned for `day`, split into Booked and Any time, each in
 *  its order. */
export function planFor(cards: readonly CardData[], day: Day): DayPlan {
  const onDay = cards.filter((c) => planDay(c) === day)
  return {
    booked: onDay.filter((c) => planTime(c) != null).sort(compareBooked),
    anyTime: onDay.filter((c) => planTime(c) == null).sort(compareAnyTime),
  }
}

/** When a booked job is due to finish, in minutes after midnight; its
 *  start when it has no size. */
export function endMinutes(card: CardData): number {
  const time = planTime(card)
  return (time ? toMinutes(time) : 0) + (card.estimate_minutes ?? 0)
}

/** For each booked job that starts before the one above it is due to
 *  finish, that job above, by id. */
export function overlaps(booked: readonly CardData[]): Map<number, CardData> {
  const out = new Map<number, CardData>()
  for (let i = 1; i < booked.length; i++) {
    const time = planTime(booked[i])
    if (time && toMinutes(time) < endMinutes(booked[i - 1])) out.set(booked[i].id, booked[i - 1])
  }
  return out
}

/** The time offered when a job is dropped into Booked at `slot`: the end
 *  of the job above rounded up to 15 minutes, or 9:00 for an empty block.
 *  Above the first job it is that job's start less this job's length. */
export function prefillTime(booked: readonly CardData[], slot: number, moved: CardData): string {
  if (booked.length === 0) return '09:00'
  if (slot <= 0) {
    const first = toMinutes(planTime(booked[0]) ?? '09:00')
    return toHhmm(Math.floor((first - (moved.estimate_minutes ?? 30)) / 15) * 15)
  }
  return toHhmm(Math.ceil(endMinutes(booked[Math.min(slot, booked.length) - 1]) / 15) * 15)
}

/** The deadline, when it is a different day from the plan day. */
export function separateDueDay(card: CardData): Day | null {
  const due = dueDay(card)
  return due && due !== planDay(card) ? due : null
}

// ---------------------------------------------------------------
// Rank keys
// ---------------------------------------------------------------

// Digits and letters in ASCII order, so keys compare byte by byte.
const DIGITS = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz'

/** A key strictly between `before` and `after`; null is an open end.
 *  Never ends in the lowest digit, so there is always room below it. */
export function rankBetween(before: string | null, after: string | null): string {
  const lo = before ?? ''
  let hi = after
  let out = ''
  for (let i = 0; i < 64; i++) {
    const a = i < lo.length ? DIGITS.indexOf(lo[i]) : 0
    const b = hi != null && i < hi.length ? DIGITS.indexOf(hi[i]) : DIGITS.length
    if (b - a > 1) return out + DIGITS[(a + b) >> 1]
    out += DIGITS[a]
    if (b !== a) hi = null
  }
  return out + 'V'
}

/** The rank writes a drop makes: the moved job takes a key between its
 *  new neighbours, and any unplaced job above it takes one too, in the
 *  order shown, so the list reads the same after the write as before.
 *  `slot` is the index among the other jobs, the moved one left out. */
export function ranksForDrop(
  order: readonly CardData[],
  movedId: number,
  slot: number,
): { id: number; rank: string }[] {
  const moved = order.find((c) => c.id === movedId)
  if (!moved) return []
  const rest = order.filter((c) => c.id !== movedId)
  const index = Math.max(0, Math.min(slot, rest.length))
  const next = [...rest.slice(0, index), moved, ...rest.slice(index)]
  // The first placed key below the drop bounds every new one.
  const ceiling = next.slice(index + 1).find((c) => c.rank != null)?.rank ?? null
  const writes: { id: number; rank: string }[] = []
  let prev: string | null = null
  for (let i = 0; i <= index; i++) {
    const card = next[i]
    if (card.id === movedId || card.rank == null) {
      const rank = rankBetween(prev, ceiling)
      writes.push({ id: card.id, rank })
      prev = rank
    } else {
      prev = card.rank
    }
  }
  return writes
}
