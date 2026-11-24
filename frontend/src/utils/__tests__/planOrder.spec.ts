import { describe, expect, it } from 'vitest'
import {
  agendaFor,
  compareAnyTime,
  MAX_PLACED_BY_DROP,
  ranksBetween,
  compareBooked,
  overlaps,
  planFor,
  prefillTime,
  rankBetween,
  ranksForDrop,
  separateDueDay,
} from '@nosdesk/core/sync/views/planOrder'
import { compareDueOrder, dueOrderKey } from '@nosdesk/core/sync/views/dueOrder'
import type { CardData, Priority } from '@nosdesk/core/sync/views/types'
import { insertionAt } from '@/sync/views/useRowReorder'
import { sortCards } from '@/composables/useTicketsSort'

const DAY = '2026-11-24'

function job(id: number, plan: Partial<CardData> = {}, priority: Priority = 'none'): CardData {
  return {
    id,
    number: 4800 + id,
    title: `Job ${id}`,
    workflow_state: { id: 2, name: 'Open', category: 'backlog', color: 'gray' },
    priority,
    due_date: `${DAY}T00:00:00`,
    created_at: `2026-11-${String(id).padStart(2, '0')}T00:00:00Z`,
    updated_at: '2026-11-01T00:00:00Z',
    last_activity_at: '2026-11-01T00:00:00Z',
    ...plan,
  }
}

const ids = (cards: readonly CardData[]) => cards.map((c) => c.id)

/** The server's rule for a rank (`ticket_plan::check_rank`). */
const VALID_RANK = /^[0-9A-Za-z]{1,128}$/

describe('the order inside a day', () => {
  it('reads booked jobs by time', () => {
    const cards = [job(1, { due_time: '14:30:00' }), job(2, { due_time: '09:00:00' }), job(3, { due_time: '09:00:00' })]
    expect(ids([...cards].sort(compareBooked))).toEqual([2, 3, 1])
  })

  it('puts placed jobs first by rank, then the rest by priority and age', () => {
    const cards = [
      job(1, {}, 'low'),
      job(2, { rank: 'V' }, 'low'),
      job(3, {}, 'urgent'),
      job(4, { rank: 'F' }, 'none'),
      job(5, {}, 'urgent'),
    ]
    expect(ids([...cards].sort(compareAnyTime))).toEqual([4, 2, 3, 5, 1])
  })

  it('splits a day into Booked and Any time, by the plan day', () => {
    const cards = [
      job(1, { due_time: '10:30:00' }),
      job(2),
      job(3, { start_date: `${DAY}T00:00:00`, start_time: '09:00:00', due_date: '2026-11-27T00:00:00' }),
      job(4, { start_date: '2026-11-25T00:00:00' }),
      job(5, { due_date: null }),
    ]
    const plan = planFor(cards, DAY)
    expect(ids(plan.booked)).toEqual([3, 1])
    expect(ids(plan.anyTime)).toEqual([2])
  })

  it('names a deadline only when it is another day', () => {
    expect(separateDueDay(job(1))).toBeNull()
    expect(separateDueDay(job(2, { start_date: `${DAY}T00:00:00`, due_date: '2026-11-27T00:00:00' }))).toBe(
      '2026-11-27',
    )
  })
})

describe("the agenda's sections", () => {
  const TODAY = DAY
  const cards = [
    // Due before today: Overdue, whatever its plan day.
    job(1, { due_date: '2026-11-20T00:00:00' }, 'low'),
    job(2, { due_date: '2026-11-22T00:00:00', due_time: '09:00:00' }),
    job(3, { start_date: TODAY + 'T00:00:00', start_time: '08:00:00', due_date: '2026-11-23T00:00:00' }),
    // Today.
    job(4, { due_time: '10:00:00' }),
    job(5),
    // Started earlier, not overdue: today's Any time, its old time left behind.
    job(6, { start_date: '2026-11-20T00:00:00', start_time: '09:00:00', due_date: '2026-11-27T00:00:00' }, 'high'),
    job(7, { start_date: '2026-11-21T00:00:00', due_date: null }),
    job(8, { start_date: '2026-11-21T00:00:00', due_date: TODAY + 'T00:00:00' }, 'urgent'),
    // No date.
    job(9, { due_date: null }, 'high'),
    job(10, { due_date: null, rank: 'V' }),
    // Another day.
    job(11, { due_date: '2026-11-25T00:00:00' }),
    job(12, { start_date: '2026-11-25T00:00:00', due_date: '2026-11-30T00:00:00' }),
  ]

  it('shows today with Overdue above and No date below, each job once', () => {
    const plan = agendaFor(cards, TODAY, TODAY)
    expect(ids(plan.overdue)).toEqual([1, 2, 3])
    expect(ids(plan.booked)).toEqual([4])
    expect(ids(plan.anyTime)).toEqual([8, 6, 5, 7])
    expect(ids(plan.undated)).toEqual([10, 9])
    const all = [...plan.overdue, ...plan.booked, ...plan.anyTime, ...plan.undated]
    expect(new Set(ids(all)).size).toBe(all.length)
  })

  it('counts a job as overdue by its due day, not its plan day', () => {
    const plan = agendaFor(cards, TODAY, TODAY)
    expect(ids(plan.overdue)).not.toContain(6)
    expect(ids(plan.overdue)).not.toContain(7)
    expect(ids(plan.booked)).not.toContain(3)
  })

  it('shows only the day itself on any other day', () => {
    const tomorrow = agendaFor(cards, '2026-11-25', TODAY)
    expect(tomorrow.overdue).toEqual([])
    expect(tomorrow.undated).toEqual([])
    expect(ids([...tomorrow.booked, ...tomorrow.anyTime])).toEqual([11, 12])
    const past = agendaFor(cards, '2026-11-22', TODAY)
    expect(past.overdue).toEqual([])
    expect(ids(past.booked)).toEqual([2])
  })
})

describe('booked times', () => {
  const booked = [
    job(1, { due_time: '09:00:00', estimate_minutes: 120 }),
    job(2, { due_time: '10:30:00', estimate_minutes: 30 }),
    job(3, { due_time: '13:00:00' }),
  ]

  it('marks a job that starts before the one above is due to finish', () => {
    const marks = overlaps(booked)
    expect([...marks.keys()]).toEqual([2])
    expect(marks.get(2)?.id).toBe(1)
  })

  it('offers the end of the job above, rounded up to the quarter hour', () => {
    const moved = job(9, { estimate_minutes: 30 })
    expect(prefillTime([], 0, moved)).toBe('09:00')
    expect(prefillTime(booked, 1, moved)).toBe('11:00')
    expect(prefillTime(booked, 2, moved)).toBe('11:00')
    expect(prefillTime(booked, 3, moved)).toBe('13:00')
    expect(prefillTime(booked, 0, moved)).toBe('08:30')
  })
})

describe('rank keys', () => {
  it('makes a key strictly between two others, never ending in the lowest digit', () => {
    const cases: [string | null, string | null][] = [
      [null, null],
      [null, 'V'],
      ['V', null],
      ['A', 'B'],
      ['V', 'V1'],
      ['0V', '1'],
      ['zz', null],
      [null, '01'],
      ['a0', 'a1'],
      [null, 'a0'],
      [null, '001'],
    ]
    for (const [lo, hi] of cases) {
      const key = rankBetween(lo, hi)
      expect(key).not.toBeNull()
      if (lo != null) expect(key! > lo).toBe(true)
      if (hi != null) expect(key! < hi).toBe(true)
      expect(key!.endsWith('0')).toBe(false)
      expect(key).toMatch(VALID_RANK)
    }
  })

  it('says when there is no room: equal ends, ends out of order, or nothing below', () => {
    expect(rankBetween('V', 'V')).toBeNull()
    expect(rankBetween('k', 'V')).toBeNull()
    expect(rankBetween(null, '0')).toBeNull()
    expect(rankBetween(null, '00')).toBeNull()
    expect(rankBetween('a', 'a0')).toBeNull()
  })

  it('gives two people placing between the same neighbours different keys', () => {
    const keys = new Set(Array.from({ length: 20 }, () => rankBetween(null, null)))
    expect(keys.size).toBeGreaterThan(1)
  })

  it('keeps finding room when jobs are dropped into the same gap again and again', () => {
    let lo = 'V'
    const hi = 'W'
    for (let i = 0; i < 200; i++) {
      const key = rankBetween(lo, hi)!
      expect(key > lo && key < hi).toBe(true)
      expect(key).toMatch(VALID_RANK)
      lo = key
    }
  })

  it('spreads many keys evenly and short, each above the one before', () => {
    const keys = ranksBetween(null, null, 500)!
    expect(keys).toHaveLength(500)
    for (let i = 1; i < keys.length; i++) expect(keys[i] > keys[i - 1]).toBe(true)
    expect(Math.max(...keys.map((k) => k.length))).toBeLessThanOrEqual(3)
    const between = ranksBetween('F', 'G', 500)!
    for (let i = 1; i < between.length; i++) expect(between[i] > between[i - 1]).toBe(true)
    expect(between[0] > 'F' && between[499] < 'G').toBe(true)
  })
})

describe('the writes a drop makes', () => {
  /** The list after the writes, in its order. */
  function after(order: CardData[], writes: { id: number; rank: string }[]): number[] {
    const next = order.map((c) => {
      const w = writes.find((x) => x.id === c.id)
      return w ? { ...c, rank: w.rank } : c
    })
    return ids(next.sort(compareAnyTime))
  }

  it('writes one rank for a drop among placed jobs', () => {
    const order = [job(1, { rank: 'F' }), job(2, { rank: 'V' }), job(3, { rank: 'k' })]
    const writes = ranksForDrop(order, 3, 1)!
    expect(writes.map((w) => w.id)).toEqual([3])
    expect(writes[0].rank > 'F' && writes[0].rank < 'V').toBe(true)
    expect(after(order, writes)).toEqual([1, 3, 2])
  })

  it('places the unplaced jobs above the drop too, in the order shown', () => {
    const order = [job(1, { rank: 'V' }), job(2, {}, 'urgent'), job(3, {}, 'high'), job(4, {}, 'low')]
    const writes = ranksForDrop(order, 4, 2)!
    expect(writes.map((w) => w.id)).toEqual([2, 4])
    expect(after(order, writes)).toEqual([1, 2, 4, 3])
    for (const w of writes) expect(w.rank).toMatch(VALID_RANK)
  })

  it('places a job at the top of an untouched day with one write', () => {
    const order = [job(1, {}, 'urgent'), job(2, {}, 'high'), job(3, {}, 'low')]
    const writes = ranksForDrop(order, 3, 0)!
    expect(writes.map((w) => w.id)).toEqual([3])
    expect(after(order, writes)).toEqual([3, 1, 2])
  })

  it('keys the jobs below again when the neighbours tie', () => {
    const order = [job(1, { rank: 'V' }), job(2, { rank: 'V' }), job(3, { rank: 'k' }), job(9)]
    const writes = ranksForDrop(order, 9, 1)!
    expect(writes.map((w) => w.id)).toEqual([9, 2])
    expect(after(order, writes)).toEqual([1, 9, 2, 3])
  })

  it('finds room under a key with nothing below it', () => {
    const order = [job(1, { rank: '0' }), job(2, { rank: 'V' }), job(9)]
    const writes = ranksForDrop(order, 9, 0)!
    expect(writes.map((w) => w.id)).toEqual([9, 1])
    expect(after(order, writes)).toEqual([9, 1, 2])
  })

  it('places a long run of unplaced jobs with keys that only go up', () => {
    const order = Array.from({ length: MAX_PLACED_BY_DROP }, (_, i) => job(i + 1))
    const writes = ranksForDrop(order, MAX_PLACED_BY_DROP, MAX_PLACED_BY_DROP - 1)!
    expect(writes).toHaveLength(MAX_PLACED_BY_DROP)
    for (let i = 1; i < writes.length; i++) expect(writes[i].rank > writes[i - 1].rank).toBe(true)
    expect(after(order, writes)).toEqual(ids(order))
  })

  it('refuses a drop that would place more jobs than one drop may', () => {
    const order = Array.from({ length: 500 }, (_, i) => job(i + 1))
    expect(ranksForDrop(order, 500, 499)).toBeNull()
    expect(ranksForDrop(order, 500, 0)).toEqual([{ id: 500, rank: expect.stringMatching(VALID_RANK) }])
  })

  it('writes nothing for a job that is not in the list', () => {
    expect(ranksForDrop([job(1)], 9, 0)).toEqual([])
  })
})

describe('where a drag lands', () => {
  const rows = [
    { id: 1, top: 0, bottom: 40 },
    { id: 2, top: 40, bottom: 80 },
    { id: 3, top: 80, bottom: 120 },
  ]

  it('takes the slot from the midpoints of the other rows', () => {
    expect(insertionAt(rows, 3, 10, 120)).toEqual({ slot: 0, same: false, edge: 0 })
    expect(insertionAt(rows, 3, 50, 120)).toEqual({ slot: 1, same: false, edge: 40 })
    expect(insertionAt(rows, 1, 110, 120)).toEqual({ slot: 2, same: false, edge: 120 })
  })

  it('knows when the row would land where it already is', () => {
    expect(insertionAt(rows, 2, 50, 120).same).toBe(true)
    expect(insertionAt(rows, 2, 70, 120).same).toBe(true)
  })

  it('draws the line at the lane bottom in an empty lane', () => {
    expect(insertionAt([], 7, 5, 64)).toEqual({ slot: 0, same: false, edge: 64 })
  })
})

describe('the list sorts', () => {
  it('reads Manual as the hand-placed order, the same either way', () => {
    const cards = [job(1, {}, 'urgent'), job(2, { rank: 'k' }), job(3, { rank: 'F' })]
    expect(ids(sortCards(cards, 'rank', 'asc'))).toEqual([3, 2, 1])
    expect(ids(sortCards(cards, 'rank', 'desc'))).toEqual([3, 2, 1])
  })

  it('reads Due by day, then time, then rank, then priority', () => {
    const cards = [
      job(1, {}, 'urgent'),
      job(2, { due_time: '15:00:00' }),
      job(3, { due_time: '09:00:00' }),
      job(4, { rank: 'V' }),
      job(5, { due_date: '2026-11-23T00:00:00' }),
    ]
    const sorted = [...cards].sort((a, b) => compareDueOrder(dueOrderKey(a), dueOrderKey(b)))
    expect(ids(sorted)).toEqual([5, 3, 2, 4, 1])
  })
})
