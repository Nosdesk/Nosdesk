import { describe, expect, it } from 'vitest'
import { computed } from 'vue'
import type { ResolvedView } from '@/composables/useTicketsViewResolution'
import { sortCards, useTicketsSort } from '@/composables/useTicketsSort'
import type { CardData } from '@nosdesk/core/sync/views/types'

const view = computed(
  () => ({ shape: { sort: [{ field: 'priority', dir: 'desc' }] } }) as unknown as ResolvedView,
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
  // As instants: 3 (Nov 20 23:00 UTC) < 1 (Nov 20 23:30 UTC)
  // < 4 (Nov 21 00:00 UTC) < 2 (Nov 21 09:00 UTC). Compared as
  // text, 1 and 3 would land after 2.
  const mixed = [
    card(1, { due_date: '2026-11-21T09:30:00+10:00' }),
    card(2, { due_date: '2026-11-21T09:00:00Z' }),
    card(3, { due_date: '2026-11-20T23:00:00.000Z' }),
    card(4, { due_date: '2026-11-21T00:00:00' }),
    card(5, { due_date: null }),
    card(6, {}),
  ]

  it('orders by the instant, whatever offset each timestamp carries', () => {
    expect(ids(sortCards(mixed, 'due_date', 'asc'))).toEqual([3, 1, 4, 2, 5, 6])
  })

  it('reads a timestamp with no offset as UTC, as the Due cell shows it', () => {
    const list = [
      card(1, { due_date: '2026-11-21T00:00:00' }),
      card(2, { due_date: '2026-11-20T23:30:00Z' }),
    ]
    expect(ids(sortCards(list, 'due_date', 'asc'))).toEqual([2, 1])
  })

  it('puts tickets with no due date last in both directions', () => {
    expect(ids(sortCards(mixed, 'due_date', 'asc')).slice(-2)).toEqual([5, 6])
    expect(ids(sortCards(mixed, 'due_date', 'desc'))).toEqual([2, 4, 1, 3, 5, 6])
  })

  it('treats an empty due date as none', () => {
    const list = [
      card(1, { due_date: '' }),
      card(2, { due_date: '2026-11-21T09:00:00Z' }),
    ]
    expect(ids(sortCards(list, 'due_date', 'asc'))).toEqual([2, 1])
    expect(ids(sortCards(list, 'due_date', 'desc'))).toEqual([2, 1])
  })

  it('keeps the input order for tickets due at the same instant', () => {
    const list = [
      card(1, { due_date: '2026-11-21T10:00:00+10:00' }),
      card(2, { due_date: '2026-11-21T00:00:00Z' }),
    ]
    expect(ids(sortCards(list, 'due_date', 'asc'))).toEqual([1, 2])
    expect(ids(sortCards(list, 'due_date', 'desc'))).toEqual([1, 2])
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
      () => ({ shape: { sort: [{ field: 'due_date', dir: 'asc' }] } }) as unknown as ResolvedView,
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
