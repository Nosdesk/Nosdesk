/**
 * Sort field + direction for the tickets table. Each view lands on
 * its own ordering: the one chosen for it on this device (Display
 * menu or a header click, kept in localStorage the way columns
 * are), else the view's first sort entry.
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
import { compareDueOrder, dueOrderKey } from '@nosdesk/core/sync/views/dueOrder'

export type SortDir = 'asc' | 'desc'

/** The fields the Sort control offers, each with the direction it
 * starts in when picked. */
export const SORT_OPTIONS: ReadonlyArray<{ field: string; labelKey: string; dir: SortDir }> = [
  { field: 'due_date', labelKey: 'views-display-menu-sort-due', dir: 'asc' },
  { field: 'priority', labelKey: 'views-display-menu-sort-priority', dir: 'desc' },
  { field: 'last_activity_at', labelKey: 'views-display-menu-sort-updated', dir: 'desc' },
  { field: 'created_at', labelKey: 'views-display-menu-sort-created', dir: 'desc' },
]

const STORAGE_PREFIX = 'tickets-sort:'

function loadSort(viewId: string): { field: string; dir: SortDir } | null {
  if (typeof localStorage === 'undefined') return null
  const raw = localStorage.getItem(`${STORAGE_PREFIX}${viewId}`)
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw)
    if (typeof parsed?.field !== 'string' || !parsed.field) return null
    if (parsed.dir !== 'asc' && parsed.dir !== 'desc') return null
    return { field: parsed.field, dir: parsed.dir }
  } catch {
    return null
  }
}

function persistSort(viewId: string, field: string, dir: SortDir): void {
  if (typeof localStorage === 'undefined') return
  localStorage.setItem(`${STORAGE_PREFIX}${viewId}`, JSON.stringify({ field, dir }))
}

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
 * strings mix offsets, so comparing the text would misorder them.
 * The due and start dates are calendar days, not instants; Due has
 * its own comparator. */
const TIMESTAMP_FIELDS = new Set(['created_at', 'updated_at', 'last_activity_at', 'closed_at'])

function sortKeyOf(card: CardData, field: string): string | number | null {
  const raw = readSortField(card, field)
  if (raw == null || !TIMESTAMP_FIELDS.has(field)) return raw
  return parseDate(String(raw))?.getTime() ?? null
}

/**
 * Order cards by one field. Cards with no value (no due date, no
 * category) go last in both directions, so flipping a column never
 * buries the rows that have one under the blanks. Due breaks ties
 * by priority, then age (see `dueOrder.ts`); other fields keep the
 * input order on a tie.
 */
export function sortCards(
  cards: readonly CardData[],
  field: string,
  dir: SortDir,
): CardData[] {
  const sign = dir === 'asc' ? 1 : -1
  // Priority sorts by severity, not by its name.
  if (field === 'priority') {
    return [...cards].sort((a, b) => comparePriority(a.priority, b.priority) * sign)
  }
  if (field === 'due_date') {
    const keyed = cards.map((card) => ({ card, key: dueOrderKey(card) }))
    keyed.sort((a, b) => compareDueOrder(a.key, b.key, dir))
    return keyed.map((k) => k.card)
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
  sortDir: Ref<SortDir>
  /** Set both and remember them for the active view. */
  setSort: (field: string, dir: SortDir) => void
  /** Header click: flip the direction, or switch to the field. */
  toggleSort: (field: string) => void
  applySort: (cards: ComputedRef<CardData[]>) => ComputedRef<CardData[]>
}

export function useTicketsSort(activeView: ComputedRef<ResolvedView>): UseTicketsSort {
  function initialFor(view: ResolvedView): { field: string; dir: SortDir } {
    const seed = view.shape.sort[0]
    return loadSort(view.id) ?? {
      field: seed?.field ?? 'last_activity_at',
      dir: seed?.dir ?? 'desc',
    }
  }

  const initial = initialFor(activeView.value)
  const sortField = ref<string>(initial.field)
  const sortDir = ref<SortDir>(initial.dir)

  watch(activeView, (next) => {
    const chosen = initialFor(next)
    sortField.value = chosen.field
    sortDir.value = chosen.dir
  })

  function setSort(field: string, dir: SortDir): void {
    sortField.value = field
    sortDir.value = dir
    persistSort(activeView.value.id, field, dir)
  }

  function toggleSort(field: string): void {
    if (sortField.value === field) setSort(field, sortDir.value === 'asc' ? 'desc' : 'asc')
    else setSort(field, 'asc')
  }

  function applySort(cards: ComputedRef<CardData[]>): ComputedRef<CardData[]> {
    return computed<CardData[]>(() => sortCards(cards.value, sortField.value, sortDir.value))
  }

  return { sortField, sortDir, setSort, toggleSort, applySort }
}
