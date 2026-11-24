/**
 * Plain-language text for rule steps: what a step will do (the Actions
 * dialog and the starter rules list) and what a run did (the activity log).
 * Looks up names for statuses, people, teams and tags, so no step reads as
 * an ID once the lookups load.
 */
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'

import { catalogueOutcomeName, recordTypeName } from '@/components/admin/assetRecordTypes/recordTypeForm'
import { GROUPS_QUERY_KEY } from '@/composables/useAssignmentPickerQueries'
import { useAssetRecordTypes } from '@/composables/useAssetTimeline'
import userService from '@/services/userService'
import { groupService } from '@nosdesk/core/services/groupService'
import { useTagsStore } from '@nosdesk/core/stores/tags'
import { useWorkflowStatesStore } from '@nosdesk/core/stores/workflowStates'
import type { RuleAction } from '@nosdesk/core/types/rule'

type Config = Record<string, unknown>

export function useRuleStepText(options: {
  /** Load the lookups only while the text is on screen. */
  enabled: () => boolean
  /** People whose names the text needs: assignees, the people notify steps
   *  email and, for the log, who ran it. */
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

  const recordTypes = useAssetRecordTypes(options.enabled)

  const userName = (uuid: unknown): string | null =>
    (usersQuery.data.value ?? []).find((u) => u.uuid === uuid)?.name ?? null
  const teamName = (id: unknown): string | null =>
    (groupsQuery.data.value ?? []).find((g) => g.id === Number(id))?.name ?? null
  const statusName = (id: unknown): string | null => states.findById(Number(id))?.name ?? null
  const recordType = (slug: unknown) => recordTypes.types.value.find((x) => x.slug === slug) ?? null
  // A record type, or an outcome of the named type (any type's, for a step
  // that names none), by name.
  const typeName = (slug: unknown): string | null => {
    const type = recordType(slug)
    return type ? recordTypeName(t, type) : null
  }
  const outcomeName = (slug: unknown, key: unknown): string | null => {
    const named = slug ? recordType(slug) : null
    const types = named ? [named] : slug ? [] : recordTypes.types.value
    for (const type of types) {
      const o = type.outcomes.find((x) => x.key === key)
      if (o) return o.label ?? catalogueOutcomeName(t, type.slug, String(key))
    }
    return null
  }
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
  // Each tag name in the locale's quotation marks, so it reads as a name
  // and not as part of the sentence.
  const quotedList = (names: string[]) => names.map((name) => t('rule-step-quoted-name', { name })).join(', ')
  // A run that found every tag already in place changed nothing, so it gets
  // no line.
  const countOf = (ids: unknown) => (Array.isArray(ids) ? ids.length : 0)
  const tagText = (key: string, ids: unknown): string | null => {
    if (Array.isArray(ids) && ids.length === 0) return null
    const names = tagNames(ids)
    const count = Array.isArray(ids) ? ids.length : 0
    return names ? t(key, { tags: quotedList(names), count }) : t(`${key}-unknown`, { count })
  }

  /** What a step will do, or null for a step that does nothing when an
   *  agent applies the rule (stop_processing). */
  function planned(action: RuleAction): string | null {
    const c = (action.config ?? {}) as Config
    switch (action.kind) {
      case 'reply':
        return c.visibility === 'internal' ? t('ticket-actions-step-note') : t('ticket-actions-step-reply')
      case 'notify': {
        if (c.recipient !== 'user') return t('ticket-actions-step-notify-requester')
        const name = userName(c.user_uuid)
        return name ? t('ticket-actions-step-notify-person', { name }) : t('ticket-actions-step-notify-person-unknown')
      }
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
      case 'add_tags': {
        // A starter names its tags, some of which adding it will create.
        const named = Array.isArray(c.tag_names) ? c.tag_names.map(String) : []
        if (named.length > 0) {
          return t('ticket-actions-step-add-tags', { tags: quotedList(named), count: named.length })
        }
        return tagText('ticket-actions-step-add-tags', c.tag_ids)
      }
      case 'remove_tags':
        return tagText('ticket-actions-step-remove-tags', c.tag_ids)
      case 'set_priority':
        return t('ticket-actions-step-priority', { priority: priorityLabel(c.priority) })
      case 'assign_assets_to_requester':
        return t('ticket-actions-step-assign-assets')
      case 'lend_assets': {
        const days = Number(c.due_in_days)
        return days > 0 ? t('ticket-actions-step-lend-assets-days', { days }) : t('ticket-actions-step-lend-assets')
      }
      case 'return_requester_assets':
        return t('ticket-actions-step-return-assets')
      case 'open_asset_record': {
        const type = typeName(c.record_type)
        return type
          ? t('ticket-actions-step-open-record', { type: t('rule-step-quoted-name', { name: type }) })
          : t('ticket-actions-step-open-record-unknown')
      }
      case 'complete_asset_records': {
        const outcome = outcomeName(c.record_type, c.outcome)
        const type = c.record_type ? typeName(c.record_type) : null
        if (!outcome || (c.record_type && !type)) return t('ticket-actions-step-complete-records-unknown')
        const quoted = {
          outcome: t('rule-step-quoted-name', { name: outcome }),
          type: type ? t('rule-step-quoted-name', { name: type }) : '',
        }
        if (!type) return t('ticket-actions-step-complete-records-any', quoted)
        return c.create_if_none === true
          ? t('ticket-actions-step-complete-records-create', quoted)
          : t('ticket-actions-step-complete-records', quoted)
      }
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
      // A notify step with no one to email (no requester, or one with no
      // address) sent nothing and says why.
      case 'notify': {
        if (entry.skipped === 'no_requester') return t('admin-rules-activity-did-notify-no-requester')
        if (entry.skipped === 'no_email') return t('admin-rules-activity-did-notify-no-email')
        if (entry.recipient !== 'user') return t('admin-rules-activity-did-notify-requester')
        const name = userName(entry.user_uuid)
        return name ? t('admin-rules-activity-did-notify-person', { name }) : t('admin-rules-activity-did-notify-person-unknown')
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
      // A step that found no linked assets says so; one whose assets were
      // already where it would put them gets a count of zero.
      case 'assign_assets_to_requester': {
        const count = countOf(entry.asset_ids)
        if (count + countOf(entry.unchanged_asset_ids) + countOf(entry.synced_asset_ids) === 0) {
          return t('admin-rules-activity-did-assets-none')
        }
        return t('admin-rules-activity-did-assign-assets', { count })
      }
      case 'lend_assets': {
        const count = countOf(entry.asset_ids)
        if (count + countOf(entry.already_lent_asset_ids) === 0) return t('admin-rules-activity-did-assets-none')
        return t('admin-rules-activity-did-lend-assets', { count })
      }
      case 'return_requester_assets': {
        const loans = countOf(entry.loan_ids)
        const cleared = countOf(entry.cleared_asset_ids)
        if (loans + cleared === 0) return t('admin-rules-activity-did-return-assets-none')
        return t('admin-rules-activity-did-return-assets', { loans, cleared })
      }
      case 'open_asset_record': {
        const count = countOf(entry.asset_ids)
        if (count + countOf(entry.already_open_asset_ids) + countOf(entry.not_for_kind_asset_ids) === 0) {
          return t('admin-rules-activity-did-assets-none')
        }
        return t('admin-rules-activity-did-open-records', { count })
      }
      // A dry run lists the assets it would write a record on, with no
      // record ids, so the assets are counted.
      case 'complete_asset_records': {
        const count = countOf(entry.completed_record_ids)
        const created = countOf(entry.created_asset_ids)
        if (created === 0) return t('admin-rules-activity-did-complete-records', { count })
        if (count === 0) return t('admin-rules-activity-did-complete-records-wrote', { created })
        return t('admin-rules-activity-did-complete-records-created', { count, created })
      }
      default:
        return null
    }
  }

  return { planned, done, userName }
}
