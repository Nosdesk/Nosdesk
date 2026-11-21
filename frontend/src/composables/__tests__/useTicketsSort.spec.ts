import { beforeEach, describe, expect, it } from 'vitest'
import { computed, nextTick, ref } from 'vue'
import type { ResolvedView } from '@/composables/useTicketsViewResolution'
import { sortCards, useTicketsSort } from '@/composables/useTicketsSort'
import type { CardData } from '@nosdesk/core/sync/views/types'

beforeEach(() => localStorage.clear())

const view = computed(
  () => ({ id: 'by-priority', shape: { sort: [{ field: 'priority', dir: 'desc' }] } }) as unknown as ResolvedView,
)
const cards = computed(() =>
  (['low', 'urgent', 'none', 'high', 'medium'] as const).map(
    (priority, id) => ({ id, priority }) as unknown as CardData,
  ),
)

describe('useTicketsSort', () => {
  it('sorts priority by severity, not alphabetically', () => {
    const { applySort, sortDir } = useTicketsSort(view)
    const sorted = applySort(cards)
    expect(sorted.value.map((c) => c.priority)).toEqual(['urgent', 'high', 'medium', 'low', 'none'])
    sortDir.value = 'asc'
    expect(sorted.value.map((c) => c.priority)).toEqual(['none', 'low', 'medium', 'high', 'urgent'])
  })
})

function card(id: number, fields: Partial<CardData>): CardData {
  return { id, priority: 'none', ...fields } as unknown as CardData
}
const ids = (list: CardData[]) => list.map((c) => c.id)

describe('sorting by due date', () => {
  // Each due date is the day as written, whatever offset follows it:
  // 1 and 2 are both the 21st, 3 the 20th, 4 the 22nd.
  const mixed = [
    card(1, { due_date: '2026-11-21T09:30:00+10:00', created_at: '2026-11-01T00:00:00Z' }),
    card(2, { due_date: '2026-11-21T00:00:00Z', created_at: '2026-11-02T00:00:00Z' }),
    card(3, { due_date: '2026-11-20T23:00:00.000Z', created_at: '2026-11-03T00:00:00Z' }),
    card(4, { due_date: '2026-11-22T00:00:00-08:00', created_at: '2026-11-04T00:00:00Z' }),
    card(5, { due_date: null }),
    card(6, {}),
  ]

  it('orders by the day as written, never shifted by an offset', () => {
    expect(ids(sortCards(mixed, 'due_date', 'asc'))).toEqual([3, 1, 2, 4, 5, 6])
  })

  it('puts tickets with no due date last in both directions', () => {
    expect(ids(sortCards(mixed, 'due_date', 'asc')).slice(-2)).toEqual([5, 6])
    expect(ids(sortCards(mixed, 'due_date', 'desc'))).toEqual([4, 1, 2, 3, 5, 6])
  })

  it('treats an empty due date as none', () => {
    const list = [
      card(1, { due_date: '' }),
      card(2, { due_date: '2026-11-21T09:00:00Z' }),
    ]
    expect(ids(sortCards(list, 'due_date', 'asc'))).toEqual([2, 1])
    expect(ids(sortCards(list, 'due_date', 'desc'))).toEqual([2, 1])
  })
})

describe('sorting by any nullable field', () => {
  it('puts the blanks last in both directions', () => {
    const list = [
      card(1, { category_id: null }),
      card(2, { category_id: 7 }),
      card(3, { category_id: 3 }),
    ]
    expect(ids(sortCards(list, 'category_id', 'asc'))).toEqual([3, 2, 1])
    expect(ids(sortCards(list, 'category_id', 'desc'))).toEqual([2, 3, 1])
  })

  it('applies through the composable when the header flips direction', () => {
    const dueView = computed(
      () => ({ id: 'by-due', shape: { sort: [{ field: 'due_date', dir: 'asc' }] } }) as unknown as ResolvedView,
    )
    const list = computed(() => [
      card(1, { due_date: null }),
      card(2, { due_date: '2026-11-22T00:00:00Z' }),
      card(3, { due_date: '2026-11-21T00:00:00Z' }),
    ])
    const { applySort, toggleSort } = useTicketsSort(dueView)
    const sorted = applySort(list)
    expect(ids(sorted.value)).toEqual([3, 2, 1])
    toggleSort('due_date')
    expect(ids(sorted.value)).toEqual([2, 3, 1])
  })
})

describe('the due comparator', () => {
  it('breaks a tie on the day by priority, most urgent first, then oldest first', () => {
    const day = '2026-11-21T00:00:00'
    const list = [
      card(1, { due_date: day, priority: 'low', created_at: '2026-11-01T00:00:00Z' }),
      card(2, { due_date: day, priority: 'urgent', created_at: '2026-11-05T00:00:00Z' }),
      card(3, { due_date: day, priority: 'urgent', created_at: '2026-11-02T00:00:00Z' }),
      card(4, { due_date: '2026-11-20T00:00:00', priority: 'none', created_at: '2026-11-09T00:00:00Z' }),
    ]
    expect(ids(sortCards(list, 'due_date', 'asc'))).toEqual([4, 3, 2, 1])
  })

  it('reverses only the day when descending; the tie-breaks stay', () => {
    const list = [
      card(1, { due_date: '2026-11-21T00:00:00', priority: 'low', created_at: '2026-11-01T00:00:00Z' }),
      card(2, { due_date: '2026-11-21T00:00:00', priority: 'high', created_at: '2026-11-02T00:00:00Z' }),
      card(3, { due_date: '2026-11-25T00:00:00', priority: 'none', created_at: '2026-11-03T00:00:00Z' }),
      card(4, { due_date: null, priority: 'urgent', created_at: '2026-11-04T00:00:00Z' }),
    ]
    expect(ids(sortCards(list, 'due_date', 'desc'))).toEqual([3, 2, 1, 4])
  })
})

describe('sorting by a timestamp', () => {
  it('orders created_at by the instant, across offsets', () => {
    // 1 is Nov 20 23:30 UTC, 2 is Nov 21 09:00 UTC; as text 1 sorts last.
    const list = [
      card(1, { created_at: '2026-11-21T09:30:00+10:00' }),
      card(2, { created_at: '2026-11-21T09:00:00Z' }),
    ]
    expect(ids(sortCards(list, 'created_at', 'asc'))).toEqual([1, 2])
  })
})

describe('the chosen sort', () => {
  const activeId = ref('my-day')
  const views: Record<string, ResolvedView> = {
    'my-day': { id: 'my-day', shape: { sort: [{ field: 'due_date', dir: 'asc' }] } } as unknown as ResolvedView,
    'all-active': { id: 'all-active', shape: { sort: [{ field: 'last_activity_at', dir: 'desc' }] } } as unknown as ResolvedView,
  }
  const active = computed(() => views[activeId.value])

  beforeEach(() => {
    activeId.value = 'my-day'
  })

  it('starts on the sort the view names', () => {
    const { sortField, sortDir } = useTicketsSort(active)
    expect([sortField.value, sortDir.value]).toEqual(['due_date', 'asc'])
  })

  it('is remembered per view, on this device', async () => {
    const first = useTicketsSort(active)
    first.setSort('priority', 'desc')
    activeId.value = 'all-active'
    await nextTick()
    expect([first.sortField.value, first.sortDir.value]).toEqual(['last_activity_at', 'desc'])
    first.toggleSort('created_at')
    activeId.value = 'my-day'
    await nextTick()
    expect([first.sortField.value, first.sortDir.value]).toEqual(['priority', 'desc'])

    // A fresh mount (a reload) reads the same choices back.
    const again = useTicketsSort(active)
    expect([again.sortField.value, again.sortDir.value]).toEqual(['priority', 'desc'])
    activeId.value = 'all-active'
    await nextTick()
    expect([again.sortField.value, again.sortDir.value]).toEqual(['created_at', 'asc'])
  })

  it('ignores a stored value it cannot read', () => {
    localStorage.setItem('tickets-sort:my-day', '{"field":"due_date","dir":"sideways"}')
    const { sortField, sortDir } = useTicketsSort(active)
    expect([sortField.value, sortDir.value]).toEqual(['due_date', 'asc'])
  })
})
