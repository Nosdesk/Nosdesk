/**
 * Pinia facade over the /api/cycles endpoints.
 *
 * Per-project cache, lazy-loaded. Cache key is the project id.
 * Mutations apply to the cache optimistically (update / archive)
 * or after the network round-trip (create / complete) since
 * those depend on a server-assigned uuid + frozen snapshot.
 *
 * Cycle membership for a ticket isn't cached here; the consumer
 * calls add/remove on demand and relies on the sync engine's
 * eventual delta to pick up the cycle_ticket change.
 */
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { logger } from '@/utils/logger'
import {
  cyclesService,
  type Cycle,
  type CreateCycleBody,
  type UpdateCycleBody,
} from '@/services/cyclesService'

export const useCyclesStore = defineStore('cycles', () => {
  const cache = ref<Map<number, Cycle[]>>(new Map())
  const loading = ref<Set<number>>(new Set())
  const lastError = ref<string | null>(null)

  async function ensureLoaded(projectId: number): Promise<Cycle[]> {
    if (cache.value.has(projectId)) return cache.value.get(projectId)!
    if (loading.value.has(projectId)) {
      while (loading.value.has(projectId)) {
        await new Promise((r) => setTimeout(r, 16))
      }
      return cache.value.get(projectId) ?? []
    }
    loading.value.add(projectId)
    try {
      const rows = await cyclesService.list(projectId)
      cache.value.set(projectId, rows)
      return rows
    } catch (e) {
      logger.warn('Failed to load cycles', { projectId, error: e })
      lastError.value = e instanceof Error ? e.message : 'Failed to load cycles'
      return []
    } finally {
      loading.value.delete(projectId)
    }
  }

  function cyclesForProject(projectId: number) {
    return computed(() => cache.value.get(projectId) ?? [])
  }

  function activeCycle(projectId: number) {
    return computed(() => {
      const rows = cache.value.get(projectId) ?? []
      return rows.find((c) => c.state === 'active' && c.archived_at == null) ?? null
    })
  }

  function applyMutation(cycle: Cycle): void {
    const rows = cache.value.get(cycle.project_id)
    if (!rows) return
    const idx = rows.findIndex((c) => c.uuid === cycle.uuid)
    if (cycle.archived_at != null) {
      if (idx >= 0) rows.splice(idx, 1)
    } else if (idx >= 0) {
      rows[idx] = cycle
    } else {
      rows.push(cycle)
      rows.sort((a, b) => {
        // Active first, then planned, then completed.
        const order = { active: 0, planned: 1, completed: 2 }
        const ord = order[a.state] - order[b.state]
        if (ord !== 0) return ord
        return (a.start_at ?? '').localeCompare(b.start_at ?? '')
      })
    }
  }

  async function create(projectId: number, body: CreateCycleBody): Promise<Cycle | null> {
    try {
      const cycle = await cyclesService.create(projectId, body)
      applyMutation(cycle)
      return cycle
    } catch (e) {
      logger.warn('Cycle create failed', { projectId, body, error: e })
      lastError.value = e instanceof Error ? e.message : 'Failed to create cycle'
      return null
    }
  }

  async function update(uuid: string, body: UpdateCycleBody): Promise<Cycle | null> {
    try {
      const cycle = await cyclesService.update(uuid, body)
      applyMutation(cycle)
      return cycle
    } catch (e) {
      logger.warn('Cycle update failed', { uuid, body, error: e })
      lastError.value = e instanceof Error ? e.message : 'Failed to update cycle'
      return null
    }
  }

  async function complete(uuid: string): Promise<Cycle | null> {
    try {
      const cycle = await cyclesService.complete(uuid)
      applyMutation(cycle)
      return cycle
    } catch (e) {
      logger.warn('Cycle complete failed', { uuid, error: e })
      lastError.value = e instanceof Error ? e.message : 'Failed to complete cycle'
      return null
    }
  }

  async function archive(uuid: string): Promise<boolean> {
    const target = findByUuid(uuid)
    try {
      await cyclesService.archive(uuid)
      if (target) {
        target.archived_at = new Date().toISOString()
        applyMutation(target)
      }
      return true
    } catch (e) {
      logger.warn('Cycle archive failed', { uuid, error: e })
      lastError.value = e instanceof Error ? e.message : 'Failed to archive cycle'
      return false
    }
  }

  function findByUuid(uuid: string): Cycle | undefined {
    for (const rows of cache.value.values()) {
      const hit = rows.find((c) => c.uuid === uuid)
      if (hit) return hit
    }
    return undefined
  }

  function reset(): void {
    cache.value.clear()
    loading.value.clear()
    lastError.value = null
  }

  return {
    cache,
    lastError,
    ensureLoaded,
    cyclesForProject,
    activeCycle,
    findByUuid,
    create,
    update,
    complete,
    archive,
    addTicket: cyclesService.addTicket,
    removeTicket: cyclesService.removeTicket,
    reset,
  }
})
