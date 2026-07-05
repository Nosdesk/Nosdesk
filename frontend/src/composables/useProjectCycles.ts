/**
 * Cycles from the sync pool: the single READ home for cycle rows.
 *
 * The pool already ingests every `cycle.*` SSE event (registered
 * aggregate + generic applier), so once rows are seeded they stay
 * live across clients for free — completing a cycle on one machine
 * moves it on all of them. REST remains the write path and the
 * initial seed (`seedProjectCycles` mirrors the old store's
 * ensureLoaded); this replaces the deleted `useCyclesStore`, whose
 * per-project Map cache could only ever update the local client.
 */
import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue'
import { useAggregate } from '@nosdesk/core/sync/composables'
import * as pool from '@nosdesk/core/sync/pool'
import { cyclesService, type Cycle } from '@nosdesk/core/services/cyclesService'
import { dedupeInFlight } from '@nosdesk/core/utils/dedupeInFlight'
import { logger } from '@nosdesk/core/utils/logger'

/**
 * Cycle row as it lives in the pool. Same shape as the REST DTO;
 * fields beyond the identity set are optional because an SSE-born
 * row carries the event payload only (the seed and the lazy fetch
 * write the full row, and shallow-merge never drops known fields).
 */
export interface PoolCycle {
  id: number
  uuid: string
  project_id: number
  name: string
  state: Cycle['state']
  start_at?: string | null
  end_at?: string | null
  completion_snapshot?: Record<string, unknown> | null
  completed_at?: string | null
  archived_at?: string | null
  created_at?: string
  updated_at?: string
}

const STATE_ORDER: Record<Cycle['state'], number> = { active: 0, planned: 1, completed: 2 }

const inflightProjects = new Map<number, Promise<void>>()
const inflightWorkspace = new Map<string, Promise<void>>()

/** Seed a project's cycles into the pool (deduped in-flight; SSE
 * keeps them live afterwards, so once per session is plenty). */
export function seedProjectCycles(projectId: number): Promise<void> {
  return dedupeInFlight(inflightProjects, projectId, async () => {
    try {
      const rows = await cyclesService.list(projectId)
      for (const c of rows) pool.upsert('cycle', c.id, { ...c })
    } catch (e) {
      logger.warn('Failed to seed project cycles', { projectId, error: e })
    }
  })
}

/** Seed the workspace's active cycles (the projects-list glance). */
export function seedActiveCycles(): Promise<void> {
  return dedupeInFlight(inflightWorkspace, 'active', async () => {
    try {
      const rows = await cyclesService.listWorkspace(['active'])
      for (const c of rows) pool.upsert('cycle', c.id, { ...c })
    } catch (e) {
      logger.warn('Failed to seed active cycles', { error: e })
    }
  })
}

export function sortCycles(rows: PoolCycle[]): PoolCycle[] {
  return rows.sort((a, b) => {
    const ord = STATE_ORDER[a.state] - STATE_ORDER[b.state]
    if (ord !== 0) return ord
    return (a.start_at ?? '9999').localeCompare(b.start_at ?? '9999')
  })
}

export function useProjectCycles(projectId: MaybeRefOrGetter<number>): {
  cycles: ComputedRef<PoolCycle[]>
  activeCycle: ComputedRef<PoolCycle | null>
  seed: () => Promise<void>
} {
  const all = useAggregate<PoolCycle>('cycle')

  const cycles = computed<PoolCycle[]>(() => {
    const pid = toValue(projectId)
    return sortCycles(all.value.filter((c) => c.project_id === pid && c.archived_at == null))
  })

  const activeCycle = computed<PoolCycle | null>(
    () => cycles.value.find((c) => c.state === 'active') ?? null,
  )

  return {
    cycles,
    activeCycle,
    seed: () => seedProjectCycles(toValue(projectId)),
  }
}

/** Find a pooled cycle by uuid (route params carry uuids; the pool
 * keys cycles by integer id). */
export function findPoolCycleByUuid(uuid: string): PoolCycle | null {
  for (const row of pool.iterate<PoolCycle>('cycle')) {
    if (row.uuid === uuid) return row
  }
  return null
}
