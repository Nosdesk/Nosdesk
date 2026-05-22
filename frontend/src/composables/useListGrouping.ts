/**
 * Dataset-agnostic group-by composable for list views.
 *
 * Tickets, assets, and users all benefit from the same grouping
 * UX: a per-axis bucket layout with collapsible group headers
 * and item counts, persisted per view so each saved view carries
 * its own preference. The data shape differs per dataset (a
 * ticket has `workflow_state`, an asset has `kind` + `location`,
 * a user has `role` + `group`), but the bucket-and-collapse
 * machinery is identical.
 *
 * Consumers declare an array of GroupAxisDef<T> describing each
 * axis: how to derive a bucket key + label from an item, and
 * optionally how to sort buckets. The composable wires up state,
 * persistence, the `buckets()` projector, and the toggle helpers.
 *
 * Storage layout (localStorage):
 *   {namespace}-group-by:{viewId}        -> axis key
 *   {namespace}-group-collapsed:{viewId} -> JSON Set<bucketKey>
 *
 * The `none` axis is built-in and means "flat list, no grouping".
 * Consumers don't declare it; it's always available in the picker.
 */
import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'

export interface GroupBucket<T> {
  key: string
  label: string
  items: T[]
}

/** Declarative axis. Generic over the item type. `bucketFor`
 *  closes over any reactive state the consumer needs (eg. a user
 *  directory for assignee axis), which keeps this composable
 *  unaware of cross-cutting concerns like SSE caches. */
export interface GroupAxisDef<T> {
  /** Stable axis identifier (eg. 'status', 'kind', 'role'). */
  key: string
  /** Fluent key for the axis's display name. */
  labelKey: string
  /** Derive a bucket key + display label from one item. The key
   *  is the dedup target; the label drives the group header. */
  bucketFor: (item: T) => { key: string; label: string }
  /** Optional sort key for buckets. Return numbers when you want
   *  a stable ordering (eg. priority severity); return strings
   *  to fall back to localeCompare. Defaults to the bucket key
   *  itself. */
  sortBy?: (bucketKey: string, bucketLabel: string) => number | string
}

export interface UseListGroupingOptions<T> {
  axes: GroupAxisDef<T>[]
  /** Per-dataset prefix so 'tickets', 'assets', 'users' don't
   *  share the same localStorage key. */
  storageNamespace: string
  /** Per-view scope id getter. Each view (built-in or saved)
   *  carries its own grouping preference + collapse state. Pass
   *  a static 'default' if the consumer doesn't have saved views
   *  yet. */
  getViewId: () => string
  /** Translate function (Fluent's $t). Used to label the
   *  built-in 'None' option in the picker. */
  t: (key: string, args?: Record<string, string | number>) => string
}

export interface GroupOption {
  key: string
  label: string
}

export interface UseListGrouping<T> {
  groupBy: Ref<string>
  setGroupBy: (axisKey: string) => void
  /** Project a reactive item list into buckets under the current
   *  axis. Empty array when groupBy is 'none' so consumers can
   *  branch on `buckets.value.length === 0` to render flat. */
  buckets: (items: ComputedRef<readonly T[]>) => ComputedRef<GroupBucket<T>[]>
  toggleCollapsed: (key: string) => void
  isCollapsed: (key: string) => boolean
  /** All selectable options for the picker including 'none'. */
  axisOptions: ComputedRef<GroupOption[]>
}

export const NONE_AXIS_KEY = 'none'

function groupByStorageKey(namespace: string, viewId: string): string {
  return `${namespace}-group-by:${viewId}`
}

function collapsedStorageKey(namespace: string, viewId: string): string {
  return `${namespace}-group-collapsed:${viewId}`
}

function loadGroupBy(
  namespace: string,
  viewId: string,
  validKeys: Set<string>,
): string {
  if (typeof localStorage === 'undefined') return NONE_AXIS_KEY
  const v = localStorage.getItem(groupByStorageKey(namespace, viewId))
  if (v && (v === NONE_AXIS_KEY || validKeys.has(v))) return v
  return NONE_AXIS_KEY
}

function loadCollapsed(namespace: string, viewId: string): Set<string> {
  if (typeof localStorage === 'undefined') return new Set()
  const raw = localStorage.getItem(collapsedStorageKey(namespace, viewId))
  if (!raw) return new Set()
  try {
    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) return new Set()
    return new Set(parsed.filter((x) => typeof x === 'string'))
  } catch {
    return new Set()
  }
}

export function useListGrouping<T>(
  options: UseListGroupingOptions<T>,
): UseListGrouping<T> {
  const { axes, storageNamespace, getViewId, t } = options
  const axisByKey = new Map(axes.map((a) => [a.key, a]))
  const validKeys = new Set(axes.map((a) => a.key))

  const groupBy = ref<string>(
    loadGroupBy(storageNamespace, getViewId(), validKeys),
  )
  const collapsed = ref<Set<string>>(loadCollapsed(storageNamespace, getViewId()))

  // Per-view scope: when the active view changes (saved-views
  // story), each view carries its own grouping + fold state.
  watch(
    () => getViewId(),
    (id) => {
      groupBy.value = loadGroupBy(storageNamespace, id, validKeys)
      collapsed.value = loadCollapsed(storageNamespace, id)
    },
  )

  function setGroupBy(value: string): void {
    if (value !== NONE_AXIS_KEY && !validKeys.has(value)) return
    groupBy.value = value
    if (typeof localStorage === 'undefined') return
    const key = groupByStorageKey(storageNamespace, getViewId())
    if (value === NONE_AXIS_KEY) localStorage.removeItem(key)
    else localStorage.setItem(key, value)
  }

  function persistCollapsed(): void {
    if (typeof localStorage === 'undefined') return
    const key = collapsedStorageKey(storageNamespace, getViewId())
    const ids = [...collapsed.value]
    if (ids.length === 0) localStorage.removeItem(key)
    else localStorage.setItem(key, JSON.stringify(ids))
  }

  function toggleCollapsed(key: string): void {
    const next = new Set(collapsed.value)
    if (next.has(key)) next.delete(key)
    else next.add(key)
    collapsed.value = next
    persistCollapsed()
  }

  function isCollapsed(key: string): boolean {
    return collapsed.value.has(key)
  }

  function buckets(
    items: ComputedRef<readonly T[]>,
  ): ComputedRef<GroupBucket<T>[]> {
    return computed<GroupBucket<T>[]>(() => {
      const axisKey = groupBy.value
      if (axisKey === NONE_AXIS_KEY) return []
      const axis = axisByKey.get(axisKey)
      if (!axis) return []
      const map = new Map<string, GroupBucket<T>>()
      for (const item of items.value) {
        const { key, label } = axis.bucketFor(item)
        let b = map.get(key)
        if (!b) {
          b = { key, label, items: [] }
          map.set(key, b)
        }
        b.items.push(item)
      }
      const out = [...map.values()]
      const sortFn = axis.sortBy
      if (sortFn) {
        out.sort((a, b) => {
          const ak = sortFn(a.key, a.label)
          const bk = sortFn(b.key, b.label)
          if (typeof ak === 'number' && typeof bk === 'number') return ak - bk
          return String(ak).localeCompare(String(bk))
        })
      } else {
        out.sort((a, b) => a.label.localeCompare(b.label))
      }
      return out
    })
  }

  const axisOptions = computed<GroupOption[]>(() => [
    { key: NONE_AXIS_KEY, label: t('list-grouping-none') },
    ...axes.map((a) => ({ key: a.key, label: t(a.labelKey) })),
  ])

  return {
    groupBy,
    setGroupBy,
    buckets,
    toggleCollapsed,
    isCollapsed,
    axisOptions,
  }
}
