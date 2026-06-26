/**
 * Shared facet metadata + option / summary helpers used by both
 * FilterPill (re-edit popover) and AddFilterMenu (two-stage
 * picker). Keeping the per-facet logic in one place means the
 * "+ Add filter" flow and the "click pill to edit" flow stay in
 * sync, there's only one source of truth for what a facet is
 * called, what options it offers, and how to summarise its
 * current selection.
 */
import { paletteForColor } from '@nosdesk/core/utils/workflowColors'
import { translate } from '@/i18n'
import type { CardData, Priority } from '@/sync/views/types'
import type { FilterFacet, SlaFilter } from '@/composables/useTicketsFilters'
import type { User } from '@nosdesk/core/types/user'

export interface FilterOption {
  value: string
  /** Display label. Resolved at construction time (priority / SLA
   * options pre-translate via the module-level `translate` helper;
   * dynamic options like user names use the raw upstream string). */
  label: string
  swatchClass?: string
  hint?: string
}

export interface FacetMeta {
  facet: FilterFacet
  /** Fluent key for the facet's display name. Consumers translate
   * via `useFluent().$t(labelKey)` or `translate(labelKey)` at
   * render time so the active locale wins. */
  labelKey: string
  /** When false the picker renders a text input instead of a
   * checkbox list. Only `title` uses this today. */
  multi: boolean
}

export const FACET_META: Record<FilterFacet, FacetMeta> = {
  title:    { facet: 'title',    labelKey: 'filter-facet-title',    multi: false },
  status:   { facet: 'status',   labelKey: 'filter-facet-status',   multi: true },
  priority: { facet: 'priority', labelKey: 'filter-facet-priority', multi: true },
  assignee: { facet: 'assignee', labelKey: 'filter-facet-assignee', multi: true },
  sla:      { facet: 'sla',      labelKey: 'filter-facet-sla',      multi: true },
  cycle:    { facet: 'cycle',    labelKey: 'filter-facet-cycle',    multi: true },
}

export const FACET_ORDER: FilterFacet[] = [
  'title', 'status', 'priority', 'assignee', 'sla', 'cycle',
]

function priorityOptions(): FilterOption[] {
  return [
    { value: 'urgent', label: translate('priority-urgent', undefined, 'Urgent'), swatchClass: 'bg-rose-500' },
    { value: 'high',   label: translate('priority-high',   undefined, 'High'),   swatchClass: 'bg-orange-500' },
    { value: 'medium', label: translate('priority-medium', undefined, 'Medium'), swatchClass: 'bg-amber-500' },
    { value: 'low',    label: translate('priority-low',    undefined, 'Low'),    swatchClass: 'bg-emerald-500' },
    { value: 'none',   label: translate('priority-none',   undefined, 'No priority'), swatchClass: 'bg-zinc-500' },
  ]
}

function slaOptions(): FilterOption[] {
  return [
    { value: 'breached', label: translate('sla-breached', undefined, 'Breached'), swatchClass: 'bg-rose-500' },
    { value: 'at-risk',  label: translate('sla-at-risk',  undefined, 'At risk'),  swatchClass: 'bg-amber-500' },
    { value: 'on-track', label: translate('sla-on-track', undefined, 'On track'), swatchClass: 'bg-emerald-500' },
    { value: 'paused',   label: translate('sla-paused',   undefined, 'Paused'),   swatchClass: 'bg-zinc-400' },
    { value: 'none',     label: translate('sla-none',     undefined, 'No SLA') },
  ]
}

/** Resolver for assignee uuid → user row. Reactive sources should
 * pass a getter that reads from a reactive cache (the directory
 * composable's `getUserHandle(uuid).user.value`) so option labels
 * update when the underlying user data lands. */
export type UserResolver = (uuid: string) => User | null | undefined

export function getOptionsFor(
  facet: FilterFacet,
  sourceCards: CardData[],
  resolveUser: UserResolver,
): FilterOption[] {
  if (facet === 'priority') return priorityOptions()
  if (facet === 'sla') return slaOptions()

  if (facet === 'status') {
    const seen = new Map<number, FilterOption>()
    for (const c of sourceCards) {
      if (seen.has(c.workflow_state.id)) continue
      seen.set(c.workflow_state.id, {
        value: String(c.workflow_state.id),
        label: c.workflow_state.name,
        swatchClass: paletteForColor(c.workflow_state.color).solid,
      })
    }
    return [...seen.values()].sort((a, b) => a.label.localeCompare(b.label))
  }

  if (facet === 'assignee') {
    const unassignedLabel = translate('filter-assignee-unassigned', undefined, 'Unassigned')
    const loadingLabel = translate('filter-assignee-loading', undefined, 'Loading…')
    const seen = new Map<string, FilterOption>()
    for (const c of sourceCards) {
      const uuid = c.assignee_uuid ?? ''
      if (seen.has(uuid)) continue
      if (!uuid) {
        seen.set('', { value: '', label: unassignedLabel })
        continue
      }
      const u = resolveUser(uuid)
      seen.set(uuid, { value: uuid, label: u?.name ?? loadingLabel, hint: u?.email ?? undefined })
    }
    return [...seen.values()].sort((a, b) => a.label.localeCompare(b.label))
  }

  if (facet === 'cycle') {
    const seen = new Map<number, FilterOption>()
    for (const c of sourceCards) {
      const cid = c.cycle_id
      if (cid == null) continue
      if (seen.has(cid)) continue
      seen.set(cid, {
        value: String(cid),
        label: translate('filter-cycle-option', { id: cid }, `Cycle #${cid}`),
      })
    }
    return [...seen.values()].sort((a, b) => Number(a.value) - Number(b.value))
  }

  return []
}

/** Render a short, scannable summary of the facet's current
 * selection for the pill body. At most 24 chars before we collapse
 * to "N selected". Title shows the literal query. */
export function summariseSelected(
  facet: FilterFacet,
  selected: Set<string> | string,
  options: FilterOption[],
): string {
  if (facet === 'title') {
    return typeof selected === 'string' && selected.length > 0
      ? `"${selected}"`
      : ''
  }
  if (typeof selected === 'string') return ''
  if (selected.size === 0) return ''
  if (selected.size > 2) {
    return translate('filter-summary-n-selected', { count: selected.size }, `${selected.size} selected`)
  }
  const labels: string[] = []
  for (const v of selected) {
    const opt = options.find((o) => o.value === v)
    labels.push(opt?.label ?? v)
  }
  const joined = labels.join(', ')
  if (joined.length > 32) {
    return translate('filter-summary-n-selected', { count: selected.size }, `${selected.size} selected`)
  }
  return joined
}

/** Map the facet's selection set / string to a Set<string>
 * regardless of underlying value type. Popover internals stay
 * generic. */
export function selectedAsStringSet(
  facet: FilterFacet,
  status: Set<number>,
  priority: Set<Priority>,
  assignee: Set<string>,
  sla: Set<SlaFilter>,
  cycle: Set<number>,
): Set<string> {
  if (facet === 'status') return new Set([...status].map(String))
  if (facet === 'priority') return new Set([...priority])
  if (facet === 'assignee') return new Set(assignee)
  if (facet === 'sla') return new Set([...sla])
  if (facet === 'cycle') return new Set([...cycle].map(String))
  return new Set()
}
