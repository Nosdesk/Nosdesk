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
import { comparePriority } from '@nosdesk/core/constants/ticketOptions'
import { parseDate } from '@nosdesk/core/utils/dateUtils'

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

/** Timestamp fields sort by the instant they name. Their stored
 * strings mix offsets (`Z`, `+10:00`, none at all for a day the
 * gantt wrote), so comparing the text would misorder them. */
const TIMESTAMP_FIELDS = new Set([
  'due_date',
  'start_date',
  'created_at',
  'updated_at',
  'last_activity_at',
  'closed_at',
])

function sortKeyOf(card: CardData, field: string): string | number | null {
  const raw = readSortField(card, field)
  if (raw == null || !TIMESTAMP_FIELDS.has(field)) return raw
  return parseDate(String(raw))?.getTime() ?? null
}

/**
 * Order cards by one field. Cards with no value (no due date, no
 * category) go last in both directions, so flipping a column never
 * buries the rows that have one under the blanks. Ties keep their
 * input order.
 */
export function sortCards(
  cards: readonly CardData[],
  field: string,
  dir: 'asc' | 'desc',
): CardData[] {
  const sign = dir === 'asc' ? 1 : -1
  // Priority sorts by severity, not by its name.
  if (field === 'priority') {
    return [...cards].sort((a, b) => comparePriority(a.priority, b.priority) * sign)
  }
  const keyed = cards.map((card) => ({ card, key: sortKeyOf(card, field) }))
  keyed.sort((a, b) => {
    if (a.key === b.key) return 0
    if (a.key == null) return 1
    if (b.key == null) return -1
    return (a.key < b.key ? -1 : 1) * sign
  })
  return keyed.map((k) => k.card)
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
    return computed<CardData[]>(() => sortCards(cards.value, sortField.value, sortDir.value))
  }

  return { sortField, sortDir, toggleSort, applySort }
}
