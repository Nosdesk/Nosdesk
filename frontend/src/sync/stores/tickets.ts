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
import * as pool from '@/sync/pool'
import ticketService from '@/services/ticketService'
import { logger } from '@nosdesk/core/utils/logger'
import type { Ticket } from '@nosdesk/core/types/ticket'
import type { CardWorkflowState, Priority } from '@/sync/views/types'

/**
 * Ticket row as it lands in the pool. Bootstrap denormalises
 * `workflow_state` so the kanban can render the column / colour
 * without a per-row workflow_states lookup. `workflow_state_id`
 * stays as the source of truth for writes.
 */
export interface SyncTicket {
  id: number
  /** Immutable identity, used to key the collaborative note doc so a
   *  recycled integer id can't inherit a prior ticket's cached note. */
  uuid: string
  title: string
  workflow_state: CardWorkflowState | null
  workflow_state_id: number
  priority: Priority
  /** True when the inbound mail filter flagged the source message as spam.
   *  Renders a badge in the queue; opens flagged + low-priority. */
  spam_suspected: boolean
  requester_uuid: string | null
  assignee_uuid: string | null
  category_id: number | null
  /** NULL means "not in the triage flow." The Triage saved view
   * filters on `triage_state = 'untriaged'`; channel-inbound
   * tickets default to that, member-created tickets to NULL. */
  triage_state: 'untriaged' | 'triaged' | 'rejected' | null
  due_date: string | null
  /** KB-gap signal density. Bootstrap derives the bucket from the
   * count of open knowledge_gap_signals attached to the ticket;
   * the renderer maps it to a pill colour. */
  kb_gap_signal?: 'none' | 'weak' | 'strong'
  /** Affected-devices summary; null when the ticket has no
   * device link. The renderer shows a "+N" badge with the first
   * device's name; consumers who need the full device list still
   * fetch /tickets/{id}. */
  affected_devices?: {
    count: number
    first?: { id: number; name: string; os?: string | null }
  } | null
  /** The cycle this ticket belongs to (null when triage / backlog).
   * Bootstrap denormalises from cycle_tickets so the Triage saved
   * view's `cycle_id is_empty` predicate evaluates without a join. */
  cycle_id?: number | null
  /** SLA pill payload from the backend SLA engine. Top-level fields
   *  reflect the most-urgent active timer (primary); nested `response`
   *  + `resolution` carry both timers for the preview-pane stack. */
  sla?: import('@nosdesk/core/types/sla').SlaPill | null
  /** RFC 5545 RRULE string. Closing a ticket with a rule spawns
   * the next occurrence (services/recurrence on the backend). */
  recurrence_rule?: string | null
  /** First ticket in a recurring series; null on the original. */
  recurrence_template_id?: number | null
  created_at: string
  updated_at: string
  last_activity_at: string
  closed_at?: string | null
}

function apiTicketToSync(ticket: Ticket): SyncTicket {
  const ws = ticket.workflow_state
  const cardWs: CardWorkflowState | null = ws
    ? { id: ws.id, name: ws.name, category: ws.category, color: ws.color }
    : null
  return {
    id: ticket.id,
    uuid: ticket.uuid ?? '',
    title: ticket.title,
    workflow_state: cardWs,
    workflow_state_id: ticket.workflow_state_id ?? ws?.id ?? 0,
    priority: ticket.priority,
    spam_suspected: ticket.spam_suspected ?? false,
    requester_uuid: ticket.requester_user?.uuid ?? ticket.requester ?? null,
    assignee_uuid: ticket.assignee_user?.uuid ?? ticket.assignee ?? null,
    category_id: ticket.category_id ?? null,
    triage_state: null,
    due_date: ticket.due_date ?? null,
    created_at: ticket.created,
    updated_at: ticket.modified,
    last_activity_at: ticket.modified,
    closed_at: ticket.closed_at ?? null,
    recurrence_rule: ticket.recurrence_rule ?? null,
    recurrence_template_id: ticket.recurrence_template_id ?? null,
  }
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
   * Patch a small whitelist of ticket fields the board renderers
   * write directly: assignee / priority (kanban two-axis swimlane
   * drop) and due_date (gantt drag-to-reschedule). Optimistic in the
   * same pattern as moveToWorkflowState. The whitelist exists to
   * prevent a typo in the caller from blowing away unrelated fields:
   * the sync engine's apply layer trusts the patch shape, so the gate
   * has to live here.
   */
  type KanbanPatchableField = 'assignee_uuid' | 'priority' | 'due_date'
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

  async function patchTitle(ticketId: number, title: string): Promise<void> {
    const current = useEntity<SyncTicket>('ticket', ticketId).value
    if (!current || current.title === title) return
    await dispatchOptimistic<SyncTicket>('ticket', ticketId, {
      forward: { title, last_activity_at: new Date().toISOString() },
      inverse: { title: current.title },
    })
  }

  // Sorted-by-last-activity (desc) — the default kanban order so
  // the most-recent activity floats to the top of each column.
  const byLastActivity = computed(() =>
    [...all().value].sort((a, b) =>
      b.last_activity_at.localeCompare(a.last_activity_at),
    ),
  )

  /**
   * Bulk variant of `patchKanbanFields` — applies the same patch
   * to every ticket id in turn. Skips tickets the patch would be
   * a no-op for. Returns the count of dispatched patches so
   * callers can report partial-success copy ("12 of 15 updated").
   * Used by the tickets list view's bulk-action bar.
   */
  async function bulkPatchKanbanFields(
    ticketIds: number[],
    patch: KanbanPatch,
  ): Promise<number> {
    let dispatched = 0
    for (const id of ticketIds) {
      const current = useEntity<SyncTicket>('ticket', id).value
      if (!current) continue
      const inverse: KanbanPatch = {}
      let dirty = false
      for (const k of Object.keys(patch) as KanbanPatchableField[]) {
        if (current[k] === patch[k]) continue
        ;(inverse[k] as SyncTicket[KanbanPatchableField]) =
          current[k] as SyncTicket[KanbanPatchableField]
        dirty = true
      }
      if (!dirty) continue
      await dispatchOptimistic<SyncTicket>('ticket', id, {
        forward: { ...patch, last_activity_at: new Date().toISOString() },
        inverse,
      })
      dispatched++
    }
    return dispatched
  }

  /** Load a ticket into the sync pool when it isn't there yet — e.g.
   * dragging a recent ticket onto a project board before the row has
   * been bootstrapped into the workspace aggregate. */
  async function ensureInPool(ticketId: number): Promise<SyncTicket | null> {
    const existing = useEntity<SyncTicket>('ticket', ticketId).value
    if (existing) return existing
    try {
      const fetched = await ticketService.getTicketById(ticketId)
      const row = apiTicketToSync(fetched)
      pool.upsert('ticket', ticketId, row)
      return row
    } catch (err) {
      logger.error('Failed to load ticket into sync pool', { ticketId, error: err })
      return null
    }
  }

  return {
    byId,
    all,
    byLastActivity,
    moveToWorkflowState,
    bulkMoveToWorkflowState,
    patchKanbanFields,
    patchTitle,
    bulkPatchKanbanFields,
    ensureInPool,
  }
})
