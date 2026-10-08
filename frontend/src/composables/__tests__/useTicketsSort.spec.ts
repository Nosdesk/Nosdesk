import { describe, expect, it } from 'vitest'
import { computed } from 'vue'
import type { ResolvedView } from '@/composables/useTicketsViewResolution'
import { useTicketsSort } from '@/composables/useTicketsSort'
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
