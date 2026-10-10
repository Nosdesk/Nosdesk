/**
 * What the person is told when `/api/sync/push` refuses a change they made.
 *
 * The queue has already rolled the change back by then, so every refusal is
 * something the person saw land and then undo. None is benign: the server
 * never refuses on a stale base (`base_sync_id` is last writer wins) and a
 * replayed transaction comes back as applied.
 *
 * The server's `detail` is English-only and kept for the log; the person
 * gets a message keyed by `reason`.
 */
import { translate } from '@nosdesk/core/i18n'

/** Every reason `backend/src/handlers/sync/push.rs` gives, and the message
 *  for it. A reason missing here falls back to `GENERIC_REASON`. */
export const REASON_MESSAGES: Readonly<Record<string, string>> = {
  invalid_assignee: 'sync-rejected-reason-invalid-assignee',
  forbidden: 'sync-rejected-reason-forbidden',
  ticket_merged: 'sync-rejected-reason-ticket-merged',
  approval_pending: 'sync-rejected-reason-approval-pending',
  invalid_reference: 'sync-rejected-reason-invalid-reference',
  not_found: 'sync-rejected-reason-not-found',
  internal: 'sync-rejected-reason-internal',
  // The app sent something push doesn't take. Not the person's doing.
  invalid_model_id: 'sync-rejected-reason-unsupported',
  invalid_patch: 'sync-rejected-reason-unsupported',
  unsupported_field: 'sync-rejected-reason-unsupported',
  unsupported_aggregate: 'sync-rejected-reason-unsupported',
  unsupported_op: 'sync-rejected-reason-unsupported',
  use_rest_endpoint: 'sync-rejected-reason-unsupported',
}

export const GENERIC_REASON = 'sync-rejected-reason-generic'

export function reasonMessageKey(reason: string): string {
  return Object.hasOwn(REASON_MESSAGES, reason) ? REASON_MESSAGES[reason] : GENERIC_REASON
}

/** Ticket fields and the change they name; bookkeeping fields the queue
 *  sends alongside (`last_activity_at`, the denormalised `workflow_state`)
 *  don't count. */
const TICKET_FIELD_ACTIONS: Readonly<Record<string, string>> = {
  assignee_uuid: 'sync-rejected-assign',
  workflow_state_id: 'sync-rejected-status',
  priority: 'sync-rejected-priority',
  title: 'sync-rejected-rename',
  tag_ids: 'sync-rejected-tags',
  due_date: 'sync-rejected-dates',
  start_date: 'sync-rejected-dates',
}
const BOOKKEEPING = new Set(['last_activity_at', 'workflow_state'])

/** The title for a refused change: what it tried to do, counted. */
export function actionTitleKey(aggregate: string, patch: Record<string, unknown> | undefined): string {
  if (aggregate === 'project') return 'sync-rejected-project'
  if (aggregate !== 'ticket') return 'sync-rejected-other'
  const actions = new Set(
    Object.keys(patch ?? {})
      .filter((k) => !BOOKKEEPING.has(k))
      .map((k) => TICKET_FIELD_ACTIONS[k] ?? 'sync-rejected-ticket'),
  )
  return actions.size === 1 ? [...actions][0] : 'sync-rejected-ticket'
}

export interface RejectedChange {
  aggregate: string
  patch?: Record<string, unknown>
  reason: string
}

export interface RejectionNotice {
  title: string
  message: string
}

/** One notice per kind of change and message, counting the rows in it:
 *  a bulk assign the server refuses for two tickets reads "Couldn't assign
 *  2 tickets", once. */
export function summariseRejections(changes: readonly RejectedChange[]): RejectionNotice[] {
  const groups = new Map<string, { title: string; message: string; count: number }>()
  for (const c of changes) {
    const title = actionTitleKey(c.aggregate, c.patch)
    const message = reasonMessageKey(c.reason)
    const key = `${title}\n${message}`
    const group = groups.get(key)
    if (group) group.count++
    else groups.set(key, { title, message, count: 1 })
  }
  return [...groups.values()].map(({ title, message, count }) => ({
    title: translate(title, { count }),
    message: translate(message, { count }),
  }))
}
