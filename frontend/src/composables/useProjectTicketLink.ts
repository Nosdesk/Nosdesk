import { useAggregate } from '@/sync/composables'
import * as pool from '@/sync/pool'
import { projectService } from '@/services/projectService'
import { logger } from '@/utils/logger'
import type { ProjectTicketAssoc } from '@/composables/useProjectTickets'

export function projectTicketLinkKey(projectId: number, ticketId: number): string {
  return `${projectId}:${ticketId}`
}

/**
 * Optimistically link a ticket to a project in the sync pool, then
 * confirm via REST. Used when dragging a ticket from the recent-tickets
 * sidebar onto a project board or project row.
 */
export function useProjectTicketLink() {
  const associations = useAggregate<ProjectTicketAssoc>('project_ticket')

  function isLinked(projectId: number, ticketId: number): boolean {
    return associations.value.some(
      (a) => a.project_id === projectId && a.ticket_id === ticketId,
    )
  }

  async function linkToProject(projectId: number, ticketId: number): Promise<boolean> {
    if (isLinked(projectId, ticketId)) return true

    const key = projectTicketLinkKey(projectId, ticketId)
    pool.upsert('project_ticket', key, {
      project_id: projectId,
      ticket_id: ticketId,
      display_order: 0,
    })

    try {
      await projectService.addTicketToProject(projectId, ticketId)
      return true
    } catch (err) {
      logger.error('Failed to add ticket to project', { projectId, ticketId, error: err })
      pool.remove('project_ticket', key)
      return false
    }
  }

  return { isLinked, linkToProject }
}
