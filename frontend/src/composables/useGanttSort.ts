/**
 * Per-project row-sort preference for the gantt, persisted the same way
 * the view's grouping and viewport prefs are:
 *
 *   gantt-sort-by:{projectId} -> GanttSortKey
 *
 * Defaults to `start` (the waterfall): rows follow the bars' left edges
 * so the eye reads time down and across. There is deliberately no
 * "project order" option — that order is the frozen link order nobody
 * chose; a manual mode can join the picker if real demand appears.
 */
import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'
import {
  GANTT_SORT_KEYS,
  type GanttSortKey,
} from '@/sync/views/gantt/sortCards'

export interface UseGanttSort {
  sortBy: Ref<GanttSortKey>
  setSortBy: (key: string) => void
  options: ComputedRef<{ value: GanttSortKey; label: string }[]>
}

const DEFAULT_KEY: GanttSortKey = 'start'

const LABEL_KEYS: Record<GanttSortKey, string> = {
  start: 'gantt-sort-start',
  due: 'gantt-sort-due',
  priority: 'gantt-sort-priority',
}

function storageKey(viewId: string): string {
  return `gantt-sort-by:${viewId}`
}

function load(viewId: string): GanttSortKey {
  if (typeof localStorage === 'undefined') return DEFAULT_KEY
  const v = localStorage.getItem(storageKey(viewId))
  return GANTT_SORT_KEYS.includes(v as GanttSortKey) ? (v as GanttSortKey) : DEFAULT_KEY
}

export function useGanttSort(
  getViewId: () => string,
  t: (key: string) => string,
): UseGanttSort {
  const sortBy = ref<GanttSortKey>(load(getViewId()))

  watch(
    () => getViewId(),
    (id) => {
      sortBy.value = load(id)
    },
  )

  function setSortBy(key: string): void {
    if (!GANTT_SORT_KEYS.includes(key as GanttSortKey)) return
    sortBy.value = key as GanttSortKey
    if (typeof localStorage === 'undefined') return
    const storage = storageKey(getViewId())
    if (key === DEFAULT_KEY) localStorage.removeItem(storage)
    else localStorage.setItem(storage, key)
  }

  const options = computed(() =>
    GANTT_SORT_KEYS.map((key) => ({ value: key, label: t(LABEL_KEYS[key]) })),
  )

  return { sortBy, setSortBy, options }
}
