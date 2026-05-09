/**
 * Shared facet metadata + option / summary helpers used by both
 * FilterPill (re-edit popover) and AddFilterMenu (two-stage
 * picker). Keeping the per-facet logic in one place means the
 * "+ Add filter" flow and the "click pill to edit" flow stay in
 * sync — there's only one source of truth for what a facet is
 * called, what options it offers, and how to summarise its
 * current selection.
 */
import { paletteForColor } from '@/utils/workflowColors'
import type { CardData, Priority } from '@/sync/views/types'
import type { FilterFacet, SlaFilter } from '@/composables/useTicketsFilters'
import type { User } from '@/types/user'

export interface FilterOption {
  value: string
  label: string
  swatchClass?: string
  hint?: string
}

export interface FacetMeta {
  facet: FilterFacet
  label: string
  /** When false the picker renders a text input instead of a
   * checkbox list — only `title` uses this today. */
  multi: boolean
}

export const FACET_META: Record<FilterFacet, FacetMeta> = {
  title: { facet: 'title', label: 'Title', multi: false },
  status: { facet: 'status', label: 'Status', multi: true },
  priority: { facet: 'priority', label: 'Priority', multi: true },
  assignee: { facet: 'assignee', label: 'Assignee', multi: true },
  sla: { facet: 'sla', label: 'SLA', multi: true },
  cycle: { facet: 'cycle', label: 'Cycle', multi: true },
}

export const FACET_ORDER: FilterFacet[] = [
  'title', 'status', 'priority', 'assignee', 'sla', 'cycle',
]

const PRIORITY_OPTIONS: FilterOption[] = [
  { value: 'urgent', label: 'Urgent', swatchClass: 'bg-rose-500' },
  { value: 'high', label: 'High', swatchClass: 'bg-orange-500' },
  { value: 'medium', label: 'Medium', swatchClass: 'bg-amber-500' },
  { value: 'low', label: 'Low', swatchClass: 'bg-emerald-500' },
  { value: 'none', label: 'No priority', swatchClass: 'bg-zinc-500' },
]

const SLA_OPTIONS: FilterOption[] = [
  { value: 'breached', label: 'Breached', swatchClass: 'bg-rose-500' },
  { value: 'at-risk', label: 'At risk', swatchClass: 'bg-amber-500' },
  { value: 'on-track', label: 'On track', swatchClass: 'bg-emerald-500' },
  { value: 'paused', label: 'Paused', swatchClass: 'bg-zinc-400' },
  { value: 'none', label: 'No SLA' },
]

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
  if (facet === 'priority') return PRIORITY_OPTIONS
  if (facet === 'sla') return SLA_OPTIONS

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
    const seen = new Map<string, FilterOption>()
    for (const c of sourceCards) {
      const uuid = c.assignee_uuid ?? ''
      if (seen.has(uuid)) continue
      if (!uuid) {
        seen.set('', { value: '', label: 'Unassigned' })
        continue
      }
      const u = resolveUser(uuid)
      seen.set(uuid, { value: uuid, label: u?.name ?? 'Loading…', hint: u?.email ?? undefined })
    }
    return [...seen.values()].sort((a, b) => a.label.localeCompare(b.label))
  }

  if (facet === 'cycle') {
    const seen = new Map<number, FilterOption>()
    for (const c of sourceCards) {
      const cid = c.cycle_id
      if (cid == null) continue
      if (seen.has(cid)) continue
      seen.set(cid, { value: String(cid), label: `Cycle #${cid}` })
    }
    return [...seen.values()].sort((a, b) => Number(a.value) - Number(b.value))
  }

  return []
}

/** Render a short, scannable summary of the facet's current
 * selection for the pill body — at most 24 chars before we
 * collapse to "N selected". Title shows the literal query. */
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
  if (selected.size > 2) return `${selected.size} selected`
  const labels: string[] = []
  for (const v of selected) {
    const opt = options.find((o) => o.value === v)
    labels.push(opt?.label ?? v)
  }
  const joined = labels.join(', ')
  return joined.length > 32 ? `${selected.size} selected` : joined
}

/** Map the facet's selection set / string to a Set<string>
 * regardless of underlying value type — popover internals
 * stay generic. */
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
