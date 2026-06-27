/**
 * Quick-filter chip state for the tickets list.
 *
 * One Set per facet; the predicate ANDs across facets and ORs
 * within. So "Status: Backlog,Todo + Priority: High" matches
 * (status in {Backlog, Todo}) AND (priority in {High}).
 *
 * Shape mirrors Linear / Asana filter pills: each chip is a
 * multi-select; an empty set means "no filter on this facet".
 *
 * SLA is a preset enum rather than a free set because the
 * underlying field is computed (breached / paused / on-track /
 * no-policy) — exposing the bucket directly is more useful than
 * making the user reason about pill colours.
 */
import { computed, ref, type ComputedRef, type Ref } from 'vue'
import type { CardData, Priority } from '@nosdesk/core/sync/views/types'

export type SlaFilter = 'breached' | 'at-risk' | 'on-track' | 'paused' | 'none'

/** Facet identifiers used by the AddFilterMenu and the filter
 * pill renderer. Kept as a string union so consumers can
 * exhaustively switch over them. */
export type FilterFacet = 'title' | 'status' | 'priority' | 'assignee' | 'sla' | 'cycle'

export interface UseTicketsFilters {
  title: Ref<string>
  status: Ref<Set<number>>
  priority: Ref<Set<Priority>>
  assignee: Ref<Set<string>>
  sla: Ref<Set<SlaFilter>>
  cycle: Ref<Set<number>>
  predicate: ComputedRef<(card: CardData) => boolean>
  activeCount: ComputedRef<number>
  /** Which facets currently have an active filter applied —
   * drives which pills the header renders. */
  activeFacets: ComputedRef<FilterFacet[]>
  clearAll: () => void
  clearFacet: (facet: FilterFacet) => void
  describe: ComputedRef<string[]>
}

function classifySla(card: CardData): SlaFilter {
  const sla = card.sla
  if (!sla) return 'none'
  if (sla.breached) return 'breached'
  if (sla.paused) return 'paused'
  if (sla.pill_color === 'amber') return 'at-risk'
  return 'on-track'
}

export function useTicketsFilters(): UseTicketsFilters {
  const title = ref<string>('')
  const status = ref<Set<number>>(new Set())
  const priority = ref<Set<Priority>>(new Set())
  const assignee = ref<Set<string>>(new Set())
  const sla = ref<Set<SlaFilter>>(new Set())
  const cycle = ref<Set<number>>(new Set())

  const predicate = computed<(card: CardData) => boolean>(() => {
    const t = title.value.trim().toLowerCase()
    const st = status.value
    const pr = priority.value
    const as = assignee.value
    const sl = sla.value
    const cy = cycle.value
    const anyTitle = t.length === 0
    const anyStatus = st.size === 0
    const anyPriority = pr.size === 0
    const anyAssignee = as.size === 0
    const anySla = sl.size === 0
    const anyCycle = cy.size === 0
    return (card) => {
      if (!anyTitle && !card.title.toLowerCase().includes(t)) return false
      if (!anyStatus && !st.has(card.workflow_state.id)) return false
      if (!anyPriority && !pr.has(card.priority)) return false
      if (!anyAssignee) {
        const a = card.assignee_uuid ?? ''
        if (!as.has(a)) return false
      }
      if (!anySla && !sl.has(classifySla(card))) return false
      if (!anyCycle) {
        const cid = card.cycle_id ?? -1
        if (!cy.has(cid)) return false
      }
      return true
    }
  })

  const activeFacets = computed<FilterFacet[]>(() => {
    const out: FilterFacet[] = []
    if (title.value.trim().length > 0) out.push('title')
    if (status.value.size > 0) out.push('status')
    if (priority.value.size > 0) out.push('priority')
    if (assignee.value.size > 0) out.push('assignee')
    if (sla.value.size > 0) out.push('sla')
    if (cycle.value.size > 0) out.push('cycle')
    return out
  })

  const activeCount = computed<number>(() => activeFacets.value.length)

  const describe = computed<string[]>(() => {
    const out: string[] = []
    if (title.value.trim().length > 0) out.push(`title "${title.value.trim()}"`)
    if (status.value.size > 0) out.push(`${status.value.size} status`)
    if (priority.value.size > 0) out.push(`${priority.value.size} priority`)
    if (assignee.value.size > 0) out.push(`${assignee.value.size} assignee`)
    if (sla.value.size > 0) out.push(`${sla.value.size} SLA`)
    if (cycle.value.size > 0) out.push(`${cycle.value.size} cycle`)
    return out
  })

  function clearAll(): void {
    title.value = ''
    status.value = new Set()
    priority.value = new Set()
    assignee.value = new Set()
    sla.value = new Set()
    cycle.value = new Set()
  }

  function clearFacet(facet: FilterFacet): void {
    switch (facet) {
      case 'title': title.value = ''; break
      case 'status': status.value = new Set(); break
      case 'priority': priority.value = new Set(); break
      case 'assignee': assignee.value = new Set(); break
      case 'sla': sla.value = new Set(); break
      case 'cycle': cycle.value = new Set(); break
    }
  }

  return {
    title, status, priority, assignee, sla, cycle,
    predicate, activeFacets, activeCount,
    clearAll, clearFacet, describe,
  }
}
