/**
 * Cycle writes: REST is the authority, the pool gets an optimistic
 * upsert on success so the local client is snappy, and the SSE echo
 * reconciles every other client. Replaces the deleted cycle store's
 * mutation half; error copy keeps the same i18n keys.
 */
import { ref, type Ref } from 'vue'
import * as pool from '@nosdesk/core/sync/pool'
import { logger } from '@nosdesk/core/utils/logger'
import { translate } from '@nosdesk/core/i18n'
import {
  cyclesService,
  type Cycle,
  type CreateCycleBody,
  type UpdateCycleBody,
} from '@nosdesk/core/services/cyclesService'
import { findPoolCycleByUuid } from './useProjectCycles'

export function useCycleMutations(): {
  lastError: Ref<string | null>
  create: (projectId: number, body: CreateCycleBody) => Promise<Cycle | null>
  update: (uuid: string, body: UpdateCycleBody) => Promise<Cycle | null>
  complete: (uuid: string) => Promise<Cycle | null>
  archive: (uuid: string) => Promise<boolean>
  addTicket: (cycleUuid: string, ticketId: number) => Promise<void>
  removeTicket: (cycleUuid: string, ticketId: number) => Promise<void>
} {
  const lastError = ref<string | null>(null)

  function fail(e: unknown, key: string, fallback: string, context: Record<string, unknown>): null {
    logger.warn(fallback, { ...context, error: e })
    lastError.value = e instanceof Error ? e.message : translate(key, undefined, fallback)
    return null
  }

  async function create(projectId: number, body: CreateCycleBody): Promise<Cycle | null> {
    try {
      const cycle = await cyclesService.create(projectId, body)
      pool.upsert('cycle', cycle.id, { ...cycle })
      return cycle
    } catch (e) {
      return fail(e, 'error-store-cycle-create', 'Failed to create cycle', { projectId })
    }
  }

  async function update(uuid: string, body: UpdateCycleBody): Promise<Cycle | null> {
    try {
      const cycle = await cyclesService.update(uuid, body)
      pool.upsert('cycle', cycle.id, { ...cycle })
      return cycle
    } catch (e) {
      return fail(e, 'error-store-cycle-update', 'Failed to update cycle', { uuid })
    }
  }

  async function complete(uuid: string): Promise<Cycle | null> {
    try {
      const cycle = await cyclesService.complete(uuid)
      pool.upsert('cycle', cycle.id, { ...cycle })
      return cycle
    } catch (e) {
      return fail(e, 'error-store-cycle-complete', 'Failed to complete cycle', { uuid })
    }
  }

  async function archive(uuid: string): Promise<boolean> {
    try {
      await cyclesService.archive(uuid)
      const row = findPoolCycleByUuid(uuid)
      // Archived rows stay in the pool (consumers filter on
      // archived_at), mirroring the backend's soft archive.
      if (row) pool.patch('cycle', row.id, { archived_at: new Date().toISOString() })
      return true
    } catch (e) {
      fail(e, 'error-store-cycle-archive', 'Failed to archive cycle', { uuid })
      return false
    }
  }

  return {
    lastError,
    create,
    update,
    complete,
    archive,
    addTicket: cyclesService.addTicket,
    removeTicket: cyclesService.removeTicket,
  }
}
