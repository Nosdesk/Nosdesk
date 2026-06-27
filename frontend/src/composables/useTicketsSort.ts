/**
 * Sort field + direction for the tickets table. Seeded from the
 * active view's first sort entry; switching views re-seeds so
 * each view lands on its own preferred ordering.
 *
 * `sortedCards` factory wraps a `cards` ref, returning a derived
 * sorted list. Keeping the sort step in a composable lets the
 * table component stay presentational.
 */
import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'
import type { ResolvedView } from '@/composables/useTicketsViewResolution'
import type { CardData } from '@nosdesk/core/sync/views/types'

function readSortField(card: CardData, field: string): string | number | null {
  const parts = field.split('.')
  let cursor: unknown = card
  for (const part of parts) {
    if (cursor == null || typeof cursor !== 'object') return null
    cursor = (cursor as Record<string, unknown>)[part]
  }
  if (cursor == null) return null
  if (typeof cursor === 'string' || typeof cursor === 'number') return cursor
  return JSON.stringify(cursor)
}

export interface UseTicketsSort {
  sortField: Ref<string>
  sortDir: Ref<'asc' | 'desc'>
  toggleSort: (field: string) => void
  applySort: (cards: ComputedRef<CardData[]>) => ComputedRef<CardData[]>
}

export function useTicketsSort(activeView: ComputedRef<ResolvedView>): UseTicketsSort {
  const sortField = ref<string>(activeView.value.shape.sort[0]?.field ?? 'last_activity_at')
  const sortDir = ref<'asc' | 'desc'>(activeView.value.shape.sort[0]?.dir ?? 'desc')

  watch(activeView, (next) => {
    const seed = next.shape.sort[0]
    if (seed) {
      sortField.value = seed.field
      sortDir.value = seed.dir
    }
  })

  function toggleSort(field: string): void {
    if (sortField.value === field) {
      sortDir.value = sortDir.value === 'asc' ? 'desc' : 'asc'
    } else {
      sortField.value = field
      sortDir.value = 'asc'
    }
  }

  function applySort(cards: ComputedRef<CardData[]>): ComputedRef<CardData[]> {
    return computed<CardData[]>(() => {
      const field = sortField.value
      const dir = sortDir.value === 'asc' ? 1 : -1
      return [...cards.value].sort((a, b) => {
        const av = readSortField(a, field)
        const bv = readSortField(b, field)
        if (av === bv) return 0
        if (av == null) return 1 * dir
        if (bv == null) return -1 * dir
        return av < bv ? -1 * dir : 1 * dir
      })
    })
  }

  return { sortField, sortDir, toggleSort, applySort }
}
