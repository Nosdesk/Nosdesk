/**
 * Group-by for the tickets table. None / status / priority /
 * assignee / sla / cycle. When grouping is active the consumer
 * renders group header rows between buckets; when disabled, the
 * table is flat.
 *
 * Per-view state, persisted to localStorage so each view carries
 * its own grouping preference. Delegates to the generic
 * `useListGrouping<T>` composable for the bucket-and-collapse
 * machinery; this module just declares the ticket-specific axes
 * (status / priority / assignee / sla / cycle) and stable
 * severity orderings.
 */
import { computed, type ComputedRef, type Ref } from 'vue'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { translate } from '@/i18n'
import {
  useListGrouping,
  NONE_AXIS_KEY,
  type GroupAxisDef,
  type GroupBucket,
} from '@/composables/useListGrouping'
import type { CardData, Priority } from '@nosdesk/core/sync/views/types'

export type GroupBy =
  | 'none'
  | 'status'
  | 'priority'
  | 'assignee'
  | 'sla'
  | 'cycle'

/** Re-exported here so existing TicketsTable / TicketsListView
 *  callers keep the same import path. The generic bucket carries
 *  the item array under `items`; tickets call it `cards` for
 *  domain readability. */
export interface TicketGroupBucket extends GroupBucket<CardData> {
  cards: CardData[]
}

export interface UseTicketsGrouping {
  groupBy: Ref<GroupBy>
  setGroupBy: (value: GroupBy) => void
  buckets: (cards: ComputedRef<CardData[]>) => ComputedRef<TicketGroupBucket[]>
  toggleCollapsed: (key: string) => void
  isCollapsed: (key: string) => boolean
}

// Stable severity orderings so chips and group headers always
// read top-to-bottom in severity order, regardless of bucket
// label localisation.
const PRIORITY_ORDER: Priority[] = ['urgent', 'high', 'medium', 'low', 'none']
const SLA_ORDER = ['breached', 'at-risk', 'on-track', 'paused', 'none'] as const

function priorityLabel(p: Priority): string {
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

export function useTicketsGrouping(getViewId: () => string): UseTicketsGrouping {
  const { getUser } = useUsersDirectory()

  const axes: GroupAxisDef<CardData>[] = [
    {
      key: 'status',
      labelKey: 'views-display-menu-group-status',
      bucketFor: (card) => ({
        key: `status:${card.workflow_state.id}`,
        label: card.workflow_state.name,
      }),
    },
    {
      key: 'priority',
      labelKey: 'views-display-menu-group-priority',
      bucketFor: (card) => ({
        key: `priority:${card.priority}`,
        label: priorityLabel(card.priority),
      }),
      sortBy: (bucketKey) => {
        const v = bucketKey.replace('priority:', '') as Priority
        const idx = PRIORITY_ORDER.indexOf(v)
        return idx === -1 ? 999 : idx
      },
    },
    {
      key: 'assignee',
      labelKey: 'views-display-menu-group-assignee',
      bucketFor: (card) => {
        const uuid = card.assignee_uuid ?? '__unassigned'
        const u = card.assignee_uuid ? getUser(card.assignee_uuid).value : null
        return {
          key: `assignee:${uuid}`,
          label:
            u?.name ??
            (card.assignee_uuid
              ? translate('filter-assignee-loading', undefined, 'Loading…')
              : translate('filter-assignee-unassigned', undefined, 'Unassigned')),
        }
      },
    },
    {
      key: 'sla',
      labelKey: 'views-display-menu-group-sla',
      bucketFor: (card) => {
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
      },
      sortBy: (bucketKey) => {
        const v = bucketKey.replace('sla:', '') as (typeof SLA_ORDER)[number]
        const idx = SLA_ORDER.indexOf(v)
        return idx === -1 ? 999 : idx
      },
    },
    {
      key: 'cycle',
      labelKey: 'views-display-menu-group-cycle',
      bucketFor: (card) => {
        const cid = card.cycle_id ?? '__no_cycle'
        return {
          key: `cycle:${cid}`,
          label: card.cycle_id
            ? translate('filter-cycle-option', { id: card.cycle_id }, `Cycle #${card.cycle_id}`)
            : translate('tickets-grouping-no-cycle', undefined, 'No cycle'),
        }
      },
    },
  ]

  const base = useListGrouping<CardData>({
    axes,
    storageNamespace: 'tickets',
    getViewId,
    // `translate` runs through the same Fluent bundle as
    // `fluent.$t`; using it here lets the composable be called
    // from contexts where useFluent() isn't reachable (eg. a
    // light-weight test harness).
    t: (key, args) => translate(key, args, key),
  })

  // Adapter: tickets historically returned buckets with `cards`
  // (domain-friendly) instead of the generic `items`. Keep that
  // shape so TicketsTable / TicketsListView keep working
  // unchanged; the generic `items` is also populated.
  function buckets(
    cards: ComputedRef<CardData[]>,
  ): ComputedRef<TicketGroupBucket[]> {
    const generic = base.buckets(cards)
    return computed<TicketGroupBucket[]>(() =>
      generic.value.map((b) => ({
        key: b.key,
        label: b.label,
        items: b.items,
        cards: b.items,
      })),
    )
  }

  return {
    groupBy: base.groupBy as Ref<GroupBy>,
    setGroupBy: (value) => base.setGroupBy(value === 'none' ? NONE_AXIS_KEY : value),
    buckets,
    toggleCollapsed: base.toggleCollapsed,
    isCollapsed: base.isCollapsed,
  }
}
