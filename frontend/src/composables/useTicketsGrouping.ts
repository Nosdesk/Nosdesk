/**
 * Group-by for the tickets table. None / status / priority /
 * assignee / sla / cycle. When grouping is active the consumer
 * renders group header rows between buckets; when disabled, the
 * table is flat.
 *
 * Per-view state, persisted to localStorage so each view carries
 * its own grouping preference.
 *
 * `collapsed` tracks per-bucket fold state. Defaults open;
 * persisted with the same key so reload survives.
 */
import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { translate } from '@/i18n'
import type { CardData } from '@/sync/views/types'

export type GroupBy = 'none' | 'status' | 'priority' | 'assignee' | 'sla' | 'cycle'

export interface GroupBucket {
  key: string
  label: string
  cards: CardData[]
}

const STORAGE_PREFIX = 'tickets-group-by:'
const COLLAPSED_PREFIX = 'tickets-group-collapsed:'

function storageKey(viewId: string): string {
  return `${STORAGE_PREFIX}${viewId}`
}

function collapsedKey(viewId: string): string {
  return `${COLLAPSED_PREFIX}${viewId}`
}

function loadGroupBy(viewId: string): GroupBy {
  if (typeof localStorage === 'undefined') return 'none'
  const v = localStorage.getItem(storageKey(viewId))
  if (
    v === 'status' || v === 'priority' || v === 'assignee' ||
    v === 'sla' || v === 'cycle' || v === 'none'
  ) return v
  return 'none'
}

function loadCollapsed(viewId: string): Set<string> {
  if (typeof localStorage === 'undefined') return new Set()
  const raw = localStorage.getItem(collapsedKey(viewId))
  if (!raw) return new Set()
  try {
    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) return new Set()
    return new Set(parsed.filter((x) => typeof x === 'string'))
  } catch {
    return new Set()
  }
}

export interface UseTicketsGrouping {
  groupBy: Ref<GroupBy>
  setGroupBy: (value: GroupBy) => void
  buckets: (cards: ComputedRef<CardData[]>) => ComputedRef<GroupBucket[]>
  toggleCollapsed: (key: string) => void
  isCollapsed: (key: string) => boolean
}

export function useTicketsGrouping(getViewId: () => string): UseTicketsGrouping {
  const { getUser } = useUsersDirectory()

  const groupBy = ref<GroupBy>(loadGroupBy(getViewId()))
  const collapsed = ref<Set<string>>(loadCollapsed(getViewId()))

  // Re-load both refs when the active view changes so each view
  // carries its own grouping preference + fold state.
  watch(
    () => getViewId(),
    (id) => {
      groupBy.value = loadGroupBy(id)
      collapsed.value = loadCollapsed(id)
    },
  )

  function setGroupBy(value: GroupBy): void {
    groupBy.value = value
    if (typeof localStorage !== 'undefined') {
      if (value === 'none') {
        localStorage.removeItem(storageKey(getViewId()))
      } else {
        localStorage.setItem(storageKey(getViewId()), value)
      }
    }
  }

  function persistCollapsed(): void {
    if (typeof localStorage === 'undefined') return
    const ids = [...collapsed.value]
    if (ids.length === 0) {
      localStorage.removeItem(collapsedKey(getViewId()))
    } else {
      localStorage.setItem(collapsedKey(getViewId()), JSON.stringify(ids))
    }
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

  // Stable priority order so chips and group headers always read
  // top-to-bottom in severity order.
  const PRIORITY_ORDER = ['urgent', 'high', 'medium', 'low', 'none'] as const
  const SLA_ORDER = ['breached', 'at-risk', 'on-track', 'paused', 'none'] as const

  function bucketKey(card: CardData, by: GroupBy): { key: string; label: string } {
    if (by === 'status') {
      return {
        key: `status:${card.workflow_state.id}`,
        label: card.workflow_state.name,
      }
    }
    if (by === 'priority') {
      return { key: `priority:${card.priority}`, label: priorityLabel(card.priority) }
    }
    if (by === 'assignee') {
      const uuid = card.assignee_uuid ?? '__unassigned'
      const u = card.assignee_uuid ? getUser(card.assignee_uuid).value : null
      return {
        key: `assignee:${uuid}`,
        label: u?.name ?? (card.assignee_uuid
          ? translate('filter-assignee-loading', undefined, 'Loading…')
          : translate('filter-assignee-unassigned', undefined, 'Unassigned')),
      }
    }
    if (by === 'sla') {
      const sla = card.sla
      const k = !sla
        ? 'none'
        : sla.breached
          ? 'breached'
          : sla.paused
            ? 'paused'
            : sla.pill_color === 'amber'
              ? 'at-risk'
              : 'on-track'
      return { key: `sla:${k}`, label: slaLabel(k) }
    }
    if (by === 'cycle') {
      const cid = card.cycle_id ?? '__no_cycle'
      return {
        key: `cycle:${cid}`,
        label: card.cycle_id
          ? translate('filter-cycle-option', { id: card.cycle_id }, `Cycle #${card.cycle_id}`)
          : translate('tickets-grouping-no-cycle', undefined, 'No cycle'),
      }
    }
    return { key: 'all', label: translate('tickets-grouping-all', undefined, 'All') }
  }

  function priorityLabel(p: CardData['priority']): string {
    if (p === 'urgent') return translate('priority-urgent', undefined, 'Urgent')
    if (p === 'high') return translate('priority-high', undefined, 'High')
    if (p === 'medium') return translate('priority-medium', undefined, 'Medium')
    if (p === 'low') return translate('priority-low', undefined, 'Low')
    return translate('priority-none', undefined, 'No priority')
  }

  function slaLabel(k: string): string {
    if (k === 'breached') return translate('sla-breached', undefined, 'Breached')
    if (k === 'at-risk') return translate('sla-at-risk', undefined, 'At risk')
    if (k === 'on-track') return translate('sla-on-track', undefined, 'On track')
    if (k === 'paused') return translate('sla-paused', undefined, 'Paused')
    return translate('sla-none', undefined, 'No SLA')
  }

  function bucketSortKey(by: GroupBy, key: string): number | string {
    if (by === 'priority') {
      const v = key.replace('priority:', '') as typeof PRIORITY_ORDER[number]
      const idx = PRIORITY_ORDER.indexOf(v)
      return idx === -1 ? 999 : idx
    }
    if (by === 'sla') {
      const v = key.replace('sla:', '') as typeof SLA_ORDER[number]
      const idx = SLA_ORDER.indexOf(v)
      return idx === -1 ? 999 : idx
    }
    return key
  }

  function buckets(cards: ComputedRef<CardData[]>): ComputedRef<GroupBucket[]> {
    return computed<GroupBucket[]>(() => {
      const by = groupBy.value
      if (by === 'none') return []
      const map = new Map<string, GroupBucket>()
      for (const card of cards.value) {
        const { key, label } = bucketKey(card, by)
        let b = map.get(key)
        if (!b) {
          b = { key, label, cards: [] }
          map.set(key, b)
        }
        b.cards.push(card)
      }
      return [...map.values()].sort((a, b) => {
        const ak = bucketSortKey(by, a.key)
        const bk = bucketSortKey(by, b.key)
        if (typeof ak === 'number' && typeof bk === 'number') return ak - bk
        return String(ak).localeCompare(String(bk))
      })
    })
  }

  return { groupBy, setGroupBy, buckets, toggleCollapsed, isCollapsed }
}
