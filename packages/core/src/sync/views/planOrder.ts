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
import { compareDueOrder, dueOrderKey } from './dueOrder'

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

export interface AgendaPlan extends DayPlan {
  /** Today only: jobs whose due day has passed, in due order. */
  overdue: CardData[]
  /** Today only: jobs with no date at all. */
  undated: CardData[]
}

/**
 * What the agenda shows for `day`. Every day shows its own Booked and
 * Any time jobs. Today also keeps the open work that would otherwise
 * drop out of sight, as My day's due buckets did: Overdue (a due day
 * before today, on a job not planned for today or later; in due order)
 * above the day, and No date below it. A job
 * planned for an earlier day that isn't overdue yet (started Monday, due
 * Friday) is today's work: it joins Any time, its old time left behind.
 * Each job lands in one section. `cards` is the view's set, already
 * narrowed to open jobs.
 */
export function agendaFor(cards: readonly CardData[], day: Day, today: Day): AgendaPlan {
  if (day !== today) return { ...planFor(cards, day), overdue: [], undated: [] }
  const overdue: CardData[] = []
  const booked: CardData[] = []
  const anyTime: CardData[] = []
  const undated: CardData[] = []
  for (const card of cards) {
    const due = dueDay(card)
    const plan = planDay(card)
    // Planned for today or later, a job is on that day's list even past
    // its deadline (its row says "due Mon" in the danger color).
    if (due != null && due < today && (plan == null || plan < today)) overdue.push(card)
    else if (plan == null) undated.push(card)
    else if (plan === today) (planTime(card) != null ? booked : anyTime).push(card)
    else if (plan < today) anyTime.push(card)
  }
  const dueOrder = (a: CardData, b: CardData) => compareDueOrder(dueOrderKey(a), dueOrderKey(b))
  return {
    overdue: overdue.sort(dueOrder),
    booked: booked.sort(compareBooked),
    anyTime: anyTime.sort(compareAnyTime),
    undated: undated.sort(compareAnyTime),
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

// Digits and letters in ASCII order, so keys compare byte by byte. A key
// reads as a base-62 fraction (`V` is 0.V); byte order refines that
// number's order, so a key numerically between two others also sorts
// between them.
const DIGITS = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz'
const BASE = BigInt(DIGITS.length)
const ZERO = BigInt(0)
const ONE = BigInt(1)

/** The server's longest key (`ticket_plan::MAX_RANK_LEN`). */
export const MAX_RANK_LENGTH = 128

/** The most unplaced jobs one drop will place by hand. Past it the drop
 *  writes nothing and the person is told why. */
export const MAX_PLACED_BY_DROP = 50

/** `key` as an integer of `length` digits: padded with zeros, or cut. */
function toInt(key: string, length: number): bigint {
  let n = ZERO
  for (let i = 0; i < length; i++) {
    const d = i < key.length ? DIGITS.indexOf(key[i]) : 0
    n = n * BASE + BigInt(Math.max(0, d))
  }
  return n
}

function toKey(n: bigint, length: number): string {
  let out = ''
  for (let i = 0; i < length; i++) {
    out = DIGITS[Number(n % BASE)] + out
    n /= BASE
  }
  return out
}

/**
 * `count` keys strictly between `before` and `after` (null is an open
 * end), in order and spread evenly, so later drops between them find room
 * without the keys growing much: a few hundred fit in three characters.
 * Each carries a random last digit (never `0`), so two people placing a
 * job between the same neighbours rarely write the same key; the ticket
 * id breaks any tie that is left. Null when there is no room: the ends
 * are equal or out of order, or the keys would pass the server's length.
 */
export function ranksBetween(
  before: string | null,
  after: string | null,
  count: number,
  random: () => number = Math.random,
): string[] | null {
  if (count <= 0) return []
  const lo = before ?? ''
  // The shortest length with room. Cut to it, `before` rounds down and a
  // key starts at least one step above that, so it stays above `before`;
  // a cut `after` is at most `after`, so keys below it stay below.
  for (let length = 1; length < MAX_RANK_LENGTH; length++) {
    const low = toInt(lo, length)
    const high = after == null ? toInt('1', length + 1) : toInt(after, length)
    if (high - low <= BigInt(count)) {
      // No room at this length. A longer one only helps while the ends
      // differ somewhere further along.
      if (after != null && toInt(after, MAX_RANK_LENGTH) <= toInt(lo, MAX_RANK_LENGTH)) return null
      continue
    }
    const span = high - low
    const slots = BigInt(count + 1)
    const keys: string[] = []
    for (let i = 1; i <= count; i++) {
      const n = low + (span * BigInt(i)) / slots
      keys.push(toKey(n, length) + DIGITS[1 + Math.floor(random() * (DIGITS.length - 1))])
    }
    return keys
  }
  return null
}

/** One key strictly between `before` and `after`, or null when there is
 *  no room between them. */
export function rankBetween(
  before: string | null,
  after: string | null,
  random: () => number = Math.random,
): string | null {
  return ranksBetween(before, after, 1, random)?.[0] ?? null
}

export interface RankWrite {
  id: number
  rank: string
}

/**
 * The rank writes a drop makes. `order` is the list as shown (placed jobs
 * by rank, then unplaced ones), and `slot` the drop's index among the
 * jobs other than the moved one. The moved job takes a key between its
 * new neighbours, and any unplaced job above it takes one too, in the
 * order shown, so the list reads the same after the write as before.
 * When the neighbours leave no room (equal keys, say), the jobs below
 * the drop are keyed again until one sorts after the new keys.
 *
 * Null when the drop would place more than `MAX_PLACED_BY_DROP` jobs.
 */
export function ranksForDrop(
  order: readonly CardData[],
  movedId: number,
  slot: number,
  random: () => number = Math.random,
): RankWrite[] | null {
  const moved = order.find((c) => c.id === movedId)
  if (!moved) return []
  const rest = order.filter((c) => c.id !== movedId)
  const index = Math.max(0, Math.min(slot, rest.length))
  const next = [...rest.slice(0, index), moved, ...rest.slice(index)]

  // Above the drop, the placed jobs keep their keys; the unplaced ones
  // after the last of them need one, then the moved job.
  let lastPlaced = -1
  for (let i = 0; i < index; i++) if (next[i].rank != null) lastPlaced = i
  const before = lastPlaced >= 0 ? (next[lastPlaced].rank as string) : null
  const keyed = next.slice(lastPlaced + 1, index + 1)

  // Below it, the first placed key bounds the new ones (unplaced jobs
  // sort after every key). When it leaves no room, that job is keyed
  // again too, and so on down.
  for (let j = index + 1; ; j++) {
    const after = j < next.length && next[j].rank != null ? (next[j].rank as string) : null
    if (keyed.length > MAX_PLACED_BY_DROP) return null
    const keys = ranksBetween(before, after, keyed.length, random)
    if (keys) return keyed.map((card, i) => ({ id: card.id, rank: keys[i] }))
    // Even an open end has no room: the key above is at the length limit.
    if (after == null) return []
    keyed.push(next[j])
  }
}
