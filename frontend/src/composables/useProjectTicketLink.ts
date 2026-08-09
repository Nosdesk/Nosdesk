import { useAggregate } from '@nosdesk/core/sync/composables'
import * as pool from '@nosdesk/core/sync/pool'
import { projectService } from '@nosdesk/core/services/projectService'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { logger } from '@nosdesk/core/utils/logger'
import { translate } from '@/i18n'
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
      // Tell the user. The optimistic row is rolled back above, so without this
      // the ticket simply blinks out of the board and the composer closes as
      // though it worked — the failure is invisible. Members hit this routinely:
      // linking requires agent privileges and the API answers 403.
      useToastStore().error(
        translate('project-link-ticket-failed'),
        err instanceof Error ? err.message : undefined,
      )
      return false
    }
  }

  return { isLinked, linkToProject }
}
