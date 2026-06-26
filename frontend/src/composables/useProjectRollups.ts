/**
 * Per-project ticket rollup for the projects list, derived entirely
 * from the sync pool (the list subscribes to `workspace:1`, which
 * carries every ticket). One pass over the `project_ticket` aggregate
 * joined against the ticket pool yields, per project: the total, the
 * coarse status breakdown (open / in-progress / closed buckets), and
 * the distinct assignees, so a card/row can show how far along a
 * project is and who's on it without any extra requests.
 *
 * `total` counts associations (robust even if a ticket entity hasn't
 * materialised yet); bucket + assignee data come from resolved tickets.
 */
import { computed, type ComputedRef } from 'vue'
import { useAggregate } from '@/sync/composables'
import type { SyncTicket } from '@/sync/stores/tickets'
import { coarseStatusBucket } from '@nosdesk/core/types/workflow'
import type { ProjectTicketAssoc } from '@/composables/useProjectTickets'

export interface ProjectRollup {
  total: number
  open: number
  inProgress: number
  closed: number
  /** Distinct assignee uuids across the project's tickets, first-seen order. */
  assignees: string[]
}

export function useProjectRollups(): ComputedRef<Map<number, ProjectRollup>> {
  const associations = useAggregate<ProjectTicketAssoc>('project_ticket')
  const tickets = useAggregate<SyncTicket>('ticket')

  return computed(() => {
    const ticketById = new Map<number, SyncTicket>()
    for (const t of tickets.value) ticketById.set(t.id, t)

    const rollups = new Map<number, ProjectRollup>()
    const seenAssignees = new Map<number, Set<string>>()

    for (const a of associations.value) {
      let r = rollups.get(a.project_id)
      if (!r) {
        r = { total: 0, open: 0, inProgress: 0, closed: 0, assignees: [] }
        rollups.set(a.project_id, r)
        seenAssignees.set(a.project_id, new Set())
      }
      r.total++

      const ticket = ticketById.get(a.ticket_id)
      if (!ticket) continue

      if (ticket.workflow_state) {
        const bucket = coarseStatusBucket(ticket.workflow_state.category)
        if (bucket === 'open') r.open++
        else if (bucket === 'in-progress') r.inProgress++
        else r.closed++
      }

      if (ticket.assignee_uuid) {
        const seen = seenAssignees.get(a.project_id)!
        if (!seen.has(ticket.assignee_uuid)) {
          seen.add(ticket.assignee_uuid)
          r.assignees.push(ticket.assignee_uuid)
        }
      }
    }

    return rollups
  })
}
