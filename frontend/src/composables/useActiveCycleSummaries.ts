/**
 * Active-cycle summaries for the projects list glance.
 *
 * One workspace-wide fetch of active cycles (at most one per project)
 * for their name / end date, with ticket + completed counts derived
 * from the sync pool, the list subscribes to `workspace:1`, which
 * carries every ticket, and each ticket denormalises its `cycle_id`.
 * So the whole glance costs a single request; the counts come for free
 * from the same pool the rest of the card reads.
 *
 * This is the surface that replaced the removed workspace-wide Cycles
 * view: the cross-project "what's in flight" glance now lives where you
 * actually start, on the projects list. Pinia Colada gives cache-first
 * reads + silent revalidation for the cycle list.
 */
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useAggregate } from '@/sync/composables'
import type { SyncTicket } from '@/sync/stores/tickets'
import { cyclesService, type Cycle } from '@/services/cyclesService'
import { TERMINAL_CATEGORIES } from '@nosdesk/core/types/workflow'

export interface ActiveCycleSummary {
  cycle: Cycle
  tickets: number
  completed: number
}

export const ACTIVE_CYCLES_KEY = ['cycles', 'active']

async function fetchActiveCycles(): Promise<Cycle[]> {
  // The workspace list defaults to active + planned; the glance only
  // cares about cycles currently in flight.
  return (await cyclesService.listWorkspace()).filter(
    (c) => c.state === 'active' && c.archived_at == null,
  )
}

export function useActiveCycleSummaries() {
  const query = useQuery({ key: ACTIVE_CYCLES_KEY, query: fetchActiveCycles })
  const tickets = useAggregate<SyncTicket>('ticket')

  // project_id -> summary, counts folded from the pool by cycle_id.
  const byProject = computed<Map<number, ActiveCycleSummary>>(() => {
    const cycles = query.data.value ?? []
    if (cycles.length === 0) return new Map()

    const cycleIds = new Set(cycles.map((c) => c.id))
    const totals = new Map<number, number>()
    const completed = new Map<number, number>()
    for (const t of tickets.value) {
      if (t.cycle_id == null || !cycleIds.has(t.cycle_id)) continue
      totals.set(t.cycle_id, (totals.get(t.cycle_id) ?? 0) + 1)
      if (t.workflow_state && TERMINAL_CATEGORIES.has(t.workflow_state.category)) {
        completed.set(t.cycle_id, (completed.get(t.cycle_id) ?? 0) + 1)
      }
    }

    const map = new Map<number, ActiveCycleSummary>()
    for (const cycle of cycles) {
      map.set(cycle.project_id, {
        cycle,
        tickets: totals.get(cycle.id) ?? 0,
        completed: completed.get(cycle.id) ?? 0,
      })
    }
    return map
  })

  return { byProject, status: query.status, error: query.error }
}
