/**
 * Plain-language text for rule steps: what a step will do (the Actions
 * dialog and the starter rules list) and what a run did (the activity log).
 * Looks up names for statuses, people, teams and tags, so no step reads as
 * an ID once the lookups load.
 */
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'

import { GROUPS_QUERY_KEY } from '@/composables/useAssignmentPickerQueries'
import userService from '@/services/userService'
import { groupService } from '@nosdesk/core/services/groupService'
import { useTagsStore } from '@nosdesk/core/stores/tags'
import { useWorkflowStatesStore } from '@nosdesk/core/stores/workflowStates'
import type { RuleAction } from '@nosdesk/core/types/rule'

type Config = Record<string, unknown>

export function useRuleStepText(options: {
  /** Load the lookups only while the text is on screen. */
  enabled: () => boolean
  /** People whose names the text needs: assignees and, for the log, who ran it. */
  userUuids: () => string[]
}) {
  const { $t: t } = useFluent()
  const states = useWorkflowStatesStore()
  const tags = useTagsStore()
  void states.load()

  const groupsQuery = useQuery({
    key: GROUPS_QUERY_KEY,
    query: () => groupService.getGroups(),
    enabled: options.enabled,
  })
  const usersQuery = useQuery({
    key: () => ['users', 'batch', ...options.userUuids()],
    query: () => userService.getUsersBatch(options.userUuids()),
    enabled: () => options.enabled() && options.userUuids().length > 0,
    staleTime: 5 * 60 * 1000,
  })

  const userName = (uuid: unknown): string | null =>
    (usersQuery.data.value ?? []).find((u) => u.uuid === uuid)?.name ?? null
  const teamName = (id: unknown): string | null =>
    (groupsQuery.data.value ?? []).find((g) => g.id === Number(id))?.name ?? null
  const statusName = (id: unknown): string | null => states.findById(Number(id))?.name ?? null
  const tagNames = (ids: unknown): string[] | null => {
    if (!Array.isArray(ids)) return null
    const names = ids.map((id) => tags.findById(Number(id))?.name)
    return names.every((n): n is string => !!n) ? names : null
  }
  // The backend reads `normal` as medium, so the label does too.
  const priorityLabel = (value: unknown) => {
    const v = String(value ?? 'medium')
    return t(`priority-${v === 'normal' ? 'medium' : v}`)
  }
  // A run that found every tag already in place changed nothing, so it gets
  // no line.
  const tagText = (key: string, ids: unknown): string | null => {
    if (Array.isArray(ids) && ids.length === 0) return null
    const names = tagNames(ids)
    const count = Array.isArray(ids) ? ids.length : 0
    return names ? t(key, { tags: names.join(', '), count }) : t(`${key}-unknown`, { count })
  }

  /** What a step will do, or null for a step that does nothing when an
   *  agent applies the rule (stop_processing). */
  function planned(action: RuleAction): string | null {
    const c = (action.config ?? {}) as Config
    switch (action.kind) {
      case 'reply':
        return c.visibility === 'internal' ? t('ticket-actions-step-note') : t('ticket-actions-step-reply')
      case 'set_status': {
        const status = statusName(c.workflow_state_id)
        return status ? t('ticket-actions-step-status', { status }) : t('ticket-actions-step-status-unknown')
      }
      case 'assign': {
        if (c.method === 'group') {
          const team = teamName(c.group_id)
          return team ? t('ticket-actions-step-assign-team', { team }) : t('ticket-actions-step-assign-team-unknown')
        }
        const name = userName(c.user_uuid)
        return name ? t('ticket-actions-step-assign', { name }) : t('ticket-actions-step-assign-unknown')
      }
      case 'unassign':
        return t('ticket-actions-step-unassign')
      case 'add_tags':
        return tagText('ticket-actions-step-add-tags', c.tag_ids)
      case 'remove_tags':
        return tagText('ticket-actions-step-remove-tags', c.tag_ids)
      case 'set_priority':
        return t('ticket-actions-step-priority', { priority: priorityLabel(c.priority) })
      default:
        return null
    }
  }

  /** What a run's step did, from its entry in the run's log. `action` is
   *  the rule's step at that position, when the rule still has it. */
  function done(entry: Config, action?: RuleAction): string | null {
    switch (entry.kind) {
      case 'reply': {
        const visibility = action?.kind === 'reply' ? ((action.config ?? {}) as Config).visibility : undefined
        if (visibility === 'internal') return t('admin-rules-activity-did-note')
        if (visibility === 'public') return t('admin-rules-activity-did-reply')
        return t('admin-rules-activity-did-message')
      }
      case 'set_status': {
        const status = statusName(entry.workflow_state_id)
        return status ? t('admin-rules-activity-did-status', { status }) : t('admin-rules-activity-did-status-unknown')
      }
      case 'assign': {
        const name = userName(entry.assigned_to_uuid)
        return name ? t('admin-rules-activity-did-assign', { name }) : t('admin-rules-activity-did-assign-unknown')
      }
      case 'unassign':
        return t('admin-rules-activity-did-unassign')
      case 'add_tags':
        return tagText('admin-rules-activity-did-add-tags', entry.tag_ids_added)
      case 'remove_tags':
        return tagText('admin-rules-activity-did-remove-tags', entry.tag_ids_removed)
      case 'set_priority':
        return t('admin-rules-activity-did-priority', { priority: priorityLabel(entry.priority) })
      default:
        return null
    }
  }

  return { planned, done, userName }
}
