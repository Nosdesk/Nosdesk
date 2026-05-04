/**
 * Single SyncTicket → CardData mapping. Imported anywhere a route
 * surfaces tickets through a view shape (kanban, list, calendar).
 *
 * The CardData shape is denormalised at this boundary, not in the
 * renderer — every renderer reads the same closed contract so
 * adding a ticket field flows through one helper rather than each
 * route's per-view computed.
 */
import type { SyncTicket } from '@/sync/stores/tickets'
import type { CardData } from './types'

/** Returns null when the ticket has no resolved workflow_state.
 * Renderers expect a populated workflow_state on every card; the
 * caller filters nulls out at the boundary. */
export function toCardData(ticket: SyncTicket): CardData | null {
  if (!ticket.workflow_state) return null
  return {
    id: ticket.id,
    title: ticket.title,
    workflow_state: ticket.workflow_state,
    priority: ticket.priority,
    assignee_uuid: ticket.assignee_uuid,
    requester_uuid: ticket.requester_uuid,
    due_date: ticket.due_date,
    created_at: ticket.created_at,
    updated_at: ticket.updated_at,
    last_activity_at: ticket.last_activity_at,
    category_id: ticket.category_id,
    triage_state: ticket.triage_state,
    kb_gap_signal: ticket.kb_gap_signal ?? 'none',
    affected_devices: ticket.affected_devices ?? null,
    cycle_id: ticket.cycle_id ?? null,
    sla: ticket.sla ?? null,
    recurrence_rule: ticket.recurrence_rule ?? null,
    recurrence_template_id: ticket.recurrence_template_id ?? null,
  }
}
