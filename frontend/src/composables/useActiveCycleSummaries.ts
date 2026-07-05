/**
 * Active-cycle summaries for the projects list glance.
 *
 * Active cycles come from the sync pool (seeded once per session by
 * a workspace-wide fetch; every `cycle.*` SSE event keeps the rows
 * live, so completing a cycle elsewhere updates the glance without
 * a refetch). Ticket + completed counts fold from the same pool by
 * the denormalised `cycle_id`, so the whole glance costs a single
 * request per session.
 *
 * This is the surface that replaced the removed workspace-wide
 * Cycles view: the cross-project "what's in flight" glance lives
 * where you actually start, on the projects list.
 */
import { computed } from 'vue'
import { useAggregate } from '@nosdesk/core/sync/composables'
import type { SyncTicket } from '@/sync/stores/tickets'
import { TERMINAL_CATEGORIES } from '@nosdesk/core/types/workflow'
import { seedActiveCycles, type PoolCycle } from '@/composables/useProjectCycles'

export interface ActiveCycleSummary {
  cycle: PoolCycle
  tickets: number
  completed: number
}

export function useActiveCycleSummaries() {
  // Seed is deduped and cheap; SSE keeps the rows live afterwards.
  void seedActiveCycles()
  const allCycles = useAggregate<PoolCycle>('cycle')
  const tickets = useAggregate<SyncTicket>('ticket')

  // project_id -> summary, counts folded from the pool by cycle_id.
  const byProject = computed<Map<number, ActiveCycleSummary>>(() => {
    const cycles = allCycles.value.filter(
      (c) => c.state === 'active' && c.archived_at == null,
    )
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

  return { byProject }
}
