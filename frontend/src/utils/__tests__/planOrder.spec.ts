import { describe, expect, it } from 'vitest'
import {
  compareAnyTime,
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
    ]
    for (const [lo, hi] of cases) {
      const key = rankBetween(lo, hi)
      if (lo != null) expect(key > lo).toBe(true)
      if (hi != null) expect(key < hi).toBe(true)
      expect(key.endsWith('0')).toBe(false)
      expect(key).toMatch(VALID_RANK)
    }
  })

  it('keeps finding room when jobs are dropped into the same gap again and again', () => {
    let lo = 'V'
    const hi = 'W'
    for (let i = 0; i < 200; i++) {
      const key = rankBetween(lo, hi)
      expect(key > lo && key < hi).toBe(true)
      expect(key).toMatch(VALID_RANK)
      lo = key
    }
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
    const writes = ranksForDrop(order, 3, 1)
    expect(writes).toHaveLength(1)
    expect(writes[0].id).toBe(3)
    expect(writes[0].rank > 'F' && writes[0].rank < 'V').toBe(true)
    expect(after(order, writes)).toEqual([1, 3, 2])
  })

  it('places the unplaced jobs above the drop too, in the order shown', () => {
    const order = [job(1, { rank: 'V' }), job(2, {}, 'urgent'), job(3, {}, 'high'), job(4, {}, 'low')]
    const writes = ranksForDrop(order, 4, 2)
    expect(writes.map((w) => w.id)).toEqual([2, 4])
    expect(after(order, writes)).toEqual([1, 2, 4, 3])
    for (const w of writes) expect(w.rank).toMatch(VALID_RANK)
  })

  it('places a job at the top of an untouched day with one write', () => {
    const order = [job(1, {}, 'urgent'), job(2, {}, 'high'), job(3, {}, 'low')]
    const writes = ranksForDrop(order, 3, 0)
    expect(writes.map((w) => w.id)).toEqual([3])
    expect(after(order, writes)).toEqual([3, 1, 2])
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
