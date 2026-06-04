/**
 * Active-cycle summaries for the projects list glance.
 *
 * One workspace-wide fetch of active cycles (at most one per project),
 * each enriched with its live ticket / completed counts so a project
 * card can show "which cycle is in flight and how far along". This is
 * the surface that replaced the removed workspace-wide Cycles view:
 * the cross-project glance now lives where you actually start, on the
 * projects list.
 *
 * Pinia Colada gives cache-first reads + silent SWR, so navigating
 * back to the list paints from cache and revalidates in the
 * background. A failed per-cycle stats read degrades to 0/0 for that
 * one cycle rather than dropping the whole glance.
 */
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { cyclesService, type Cycle } from '@/services/cyclesService'
import { logger } from '@/utils/logger'

export interface ActiveCycleSummary {
  cycle: Cycle
  tickets: number
  completed: number
}

export const ACTIVE_CYCLE_SUMMARIES_KEY = ['cycles', 'active-summaries']

async function fetchActiveCycleSummaries(): Promise<ActiveCycleSummary[]> {
  // The workspace list defaults to active + planned; the glance only
  // cares about cycles currently in flight.
  const active = (await cyclesService.listWorkspace()).filter(
    (c) => c.state === 'active' && c.archived_at == null,
  )
  return Promise.all(
    active.map(async (cycle) => {
      try {
        const stats = await cyclesService.stats(cycle.uuid)
        return { cycle, tickets: stats.tickets, completed: stats.completed }
      } catch (e) {
        logger.warn('Active-cycle stats failed for glance', { uuid: cycle.uuid, error: e })
        return { cycle, tickets: 0, completed: 0 }
      }
    }),
  )
}

export function useActiveCycleSummaries() {
  const query = useQuery({
    key: ACTIVE_CYCLE_SUMMARIES_KEY,
    query: fetchActiveCycleSummaries,
  })

  // project_id -> summary, for O(1) lookup per list card.
  const byProject = computed<Map<number, ActiveCycleSummary>>(() => {
    const map = new Map<number, ActiveCycleSummary>()
    for (const s of query.data.value ?? []) map.set(s.cycle.project_id, s)
    return map
  })

  return { byProject, status: query.status, error: query.error }
}
