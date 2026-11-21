/**
 * Grouping the tickets list by Due: buckets in day order with their
 * counts, re-bucketed when midnight passes in the planning zone, and
 * Due as the starting grouping for a view whose shape groups on it.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, effectScope } from 'vue'
import { setDateConfig } from '@nosdesk/core/utils/dateUtils'
import { MY_DAY_VIEW, MY_OPEN_VIEW } from '@nosdesk/core/sync/views/builtinViews'
import type { CardData } from '@nosdesk/core/sync/views/types'
import { defaultGroupByFor, useTicketsGrouping } from '@/composables/useTicketsGrouping'

vi.mock('@/composables/useUsersDirectory', () => ({
  useUsersDirectory: () => ({ getUser: () => computed(() => undefined) }),
}))
vi.mock('@/i18n', () => ({
  translate: (key: string, _args: unknown, fallback?: string) => fallback ?? key,
}))

function card(id: number, due: string | null): CardData {
  return { id, due_date: due ? `${due}T00:00:00` : null } as unknown as CardData
}

beforeEach(() => {
  localStorage.clear()
  setDateConfig({ defaultTimezone: 'Australia/Sydney' })
  vi.useFakeTimers()
})
afterEach(() => {
  vi.useRealTimers()
  setDateConfig({ defaultTimezone: 'UTC' })
})

describe('group by Due', () => {
  it('buckets in day order with counts, and moves at midnight', () => {
    // 23:58 on Saturday 21 November in Sydney (UTC+11).
    vi.setSystemTime(new Date('2026-11-21T12:58:00Z'))
    const scope = effectScope()
    scope.run(() => {
      const grouping = useTicketsGrouping(() => 'my-day', () => 'due')
      const cards = computed(() => [
        card(1, '2026-11-22'),
        card(2, null),
        card(3, '2026-11-21'),
        card(4, '2026-11-19'),
        card(5, '2026-12-01'),
        card(6, '2026-11-21'),
      ])
      const buckets = grouping.buckets(cards)
      const shape = () => buckets.value.map((b) => [b.label, b.cards.map((c) => c.id)])

      expect(grouping.groupBy.value).toBe('due')
      expect(shape()).toEqual([
        ['Overdue', [4]],
        ['Today', [3, 6]],
        ['Tomorrow', [1]],
        ['Later', [5]],
        ['No date', [2]],
      ])

      // Two minutes later it is Sunday the 22nd in Sydney.
      vi.setSystemTime(new Date('2026-11-21T13:00:00Z'))
      vi.advanceTimersByTime(60_000)
      expect(shape()).toEqual([
        ['Overdue', [3, 4, 6]],
        ['Today', [1]],
        ['Later', [5]],
        ['No date', [2]],
      ])
    })
    scope.stop()
  })

  it('starts My day grouped by Due and remembers turning it off', () => {
    expect(defaultGroupByFor(MY_DAY_VIEW.shape)).toBe('due')
    expect(defaultGroupByFor(MY_OPEN_VIEW.shape)).toBe('none')

    const scope = effectScope()
    scope.run(() => {
      const first = useTicketsGrouping(() => 'my-day', () => 'due')
      expect(first.groupBy.value).toBe('due')
      first.setGroupBy('none')
      const again = useTicketsGrouping(() => 'my-day', () => 'due')
      expect(again.groupBy.value).toBe('none')
      again.setGroupBy('due')
      expect(localStorage.getItem('tickets-group-by:my-day')).toBeNull()

      const other = useTicketsGrouping(() => 'my-open', () => 'none')
      expect(other.groupBy.value).toBe('none')
    })
    scope.stop()
  })
})
