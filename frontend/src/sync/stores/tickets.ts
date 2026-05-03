/**
 * Tickets sync facade.
 *
 * Mirrors `useSyncProjectsStore` for the ticket aggregate. The
 * kanban renderer reads through `useAggregate('ticket')`; the
 * dispatcher writes through `dispatchOptimistic` so drag-to-state
 * lands instantly in the pool and the network round-trip happens
 * in the background.
 *
 * No reactive state of its own.
 */
import { defineStore } from 'pinia'
import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue'
import { useEntity, useAggregate } from '@/sync/composables'
import { dispatchOptimistic } from '@/sync/queue'
import type { CardWorkflowState, Priority } from '@/sync/views/types'

/**
 * Ticket row as it lands in the pool. Bootstrap denormalises
 * `workflow_state` so the kanban can render the column / colour
 * without a per-row workflow_states lookup. `workflow_state_id`
 * stays as the source of truth for writes.
 */
export interface SyncTicket {
  id: number
  title: string
  workflow_state: CardWorkflowState | null
  workflow_state_id: number
  priority: Priority
  requester_uuid: string | null
  assignee_uuid: string | null
  category_id: number | null
  created_at: string
  updated_at: string
  last_activity_at: string
}

export const useSyncTicketsStore = defineStore('syncTickets', () => {
  function byId(id: MaybeRefOrGetter<number | null>): ComputedRef<SyncTicket | null> {
    return useEntity<SyncTicket>('ticket', () => toValue(id))
  }

  function all(): ComputedRef<SyncTicket[]> {
    return useAggregate<SyncTicket>('ticket')
  }

  /**
   * Move a ticket to a new workflow state. Optimistic — the
   * forward patch flips workflow_state_id (and the denormalised
   * workflow_state object) immediately; the inverse restores
   * both fields if the server rejects.
   */
  async function moveToWorkflowState(
    ticketId: number,
    targetState: CardWorkflowState,
  ): Promise<void> {
    const current = useEntity<SyncTicket>('ticket', ticketId).value
    if (!current) return
    if (current.workflow_state_id === targetState.id) return
    const previousState = current.workflow_state
    const previousStateId = current.workflow_state_id
    await dispatchOptimistic<SyncTicket>('ticket', ticketId, {
      forward: {
        workflow_state_id: targetState.id,
        workflow_state: targetState,
        last_activity_at: new Date().toISOString(),
      },
      inverse: {
        workflow_state_id: previousStateId,
        workflow_state: previousState,
      },
    })
  }

  /**
   * Bulk variant: moves several tickets in parallel. Each ticket
   * dispatches its own transaction so per-ticket rejections don't
   * cascade. Returns the count of dispatched moves so callers can
   * surface "moved 12 of 15" feedback when some fail.
   */
  async function bulkMoveToWorkflowState(
    ticketIds: number[],
    targetState: CardWorkflowState,
  ): Promise<number> {
    let dispatched = 0
    for (const id of ticketIds) {
      const current = useEntity<SyncTicket>('ticket', id).value
      if (!current || current.workflow_state_id === targetState.id) continue
      const previousState = current.workflow_state
      const previousStateId = current.workflow_state_id
      await dispatchOptimistic<SyncTicket>('ticket', id, {
        forward: {
          workflow_state_id: targetState.id,
          workflow_state: targetState,
          last_activity_at: new Date().toISOString(),
        },
        inverse: {
          workflow_state_id: previousStateId,
          workflow_state: previousState,
        },
      })
      dispatched++
    }
    return dispatched
  }

  /**
   * Patch a small whitelist of ticket fields the kanban renderer
   * writes during a two-axis swimlane drop. Optimistic in the same
   * pattern as moveToWorkflowState. The whitelist exists to prevent
   * a typo in the caller from blowing away unrelated fields: the
   * sync engine's apply layer trusts the patch shape, so the gate
   * has to live here.
   */
  type KanbanPatchableField = 'assignee_uuid' | 'priority'
  type KanbanPatch = Partial<Pick<SyncTicket, KanbanPatchableField>>

  async function patchKanbanFields(ticketId: number, patch: KanbanPatch): Promise<void> {
    const current = useEntity<SyncTicket>('ticket', ticketId).value
    if (!current) return
    const inverse: KanbanPatch = {}
    let dirty = false
    for (const k of Object.keys(patch) as KanbanPatchableField[]) {
      if (current[k] === patch[k]) continue
      ;(inverse[k] as SyncTicket[KanbanPatchableField]) = current[k] as SyncTicket[KanbanPatchableField]
      dirty = true
    }
    if (!dirty) return
    await dispatchOptimistic<SyncTicket>('ticket', ticketId, {
      forward: { ...patch, last_activity_at: new Date().toISOString() },
      inverse,
    })
  }

  // Sorted-by-last-activity (desc) — the default kanban order so
  // the most-recent activity floats to the top of each column.
  const byLastActivity = computed(() =>
    [...all().value].sort((a, b) =>
      b.last_activity_at.localeCompare(a.last_activity_at),
    ),
  )

  return {
    byId,
    all,
    byLastActivity,
    moveToWorkflowState,
    bulkMoveToWorkflowState,
    patchKanbanFields,
  }
})
