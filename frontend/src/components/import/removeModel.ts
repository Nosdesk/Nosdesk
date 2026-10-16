/**
 * Remove imported assets and previous values, kept out of the components
 * so they can be tested on their own: whether a job offers removal, the
 * dialog's sentences, each kept asset's reason line, the result line, and
 * the previous values help text.
 *
 * Removal is for asset imports that created assets, once, for 30 days
 * after the commit (`removal_closes_at`); after that, or once the prune
 * removed the details, the page says the window closed.
 */
import type {
  ImportJob,
  KeptAsset,
  KeptReason,
  RemovalPlan,
} from '@nosdesk/core/services/importService'

import { outcomes } from './reviewModel'

type Translate = (key: string, args?: Record<string, string | number>) => string

/** What a past asset import shows for removal. */
export type RemovalState = 'offer' | 'removed' | 'closed'

export function removalState(job: ImportJob, now: number = Date.now()): RemovalState | null {
  if (job.job_type !== 'assets' || job.state !== 'imported') return null
  if (job.removed_at || job.remove_result) return 'removed'
  if (!job.summary || outcomes(job.summary).created === 0) return null
  if (!job.removal_closes_at) return null
  if (job.pruned_at || Date.parse(job.removal_closes_at) <= now) return 'closed'
  return 'offer'
}

/** The group a kept asset counts in on the dialog's line, by its first
 *  reason. */
function groupOf(reason: KeptReason): string {
  switch (reason.code) {
    case 'media':
      return reason.photos > 0 ? 'photos' : 'files'
    case 'status_changed':
      return 'status-changed'
    case 'group_added':
      return 'group-added'
    default:
      return reason.code
  }
}

/** "4 edited, 3 with photos, 2 linked to tickets": the kept assets by
 *  their first reason, largest group first. */
export function keptGroups(t: Translate, kept: KeptAsset[]): string {
  const counts = new Map<string, number>()
  for (const k of kept) {
    const first = k.reasons[0]
    if (!first) continue
    const group = groupOf(first)
    counts.set(group, (counts.get(group) ?? 0) + 1)
  }
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([group, count]) => t(`import-remove-group-${group}`, { count }))
    .join(', ')
}

/** The dialog's body: what the import created, what goes and what stays,
 *  the catalog entries that go with them, and the webhook line when a
 *  webhook hears about assets. */
export function dialogMessage(
  t: Translate,
  plan: RemovalPlan,
  opts: { webhooks: boolean; list: (items: string[]) => string },
): string {
  const parts = [t('import-remove-created', { count: plan.created })]
  if (plan.deletable > 0) parts.push(t('import-remove-deletable', { count: plan.deletable }))
  if (plan.kept.length > 0) {
    parts.push(t('import-remove-kept', { count: plan.kept.length, reasons: keptGroups(t, plan.kept) }))
  }
  if (plan.gone > 0) parts.push(t('import-remove-gone', { count: plan.gone }))
  const made = [
    plan.catalog.manufacturers > 0
      ? t('import-review-count-manufacturers', { count: plan.catalog.manufacturers })
      : '',
    plan.catalog.models > 0 ? t('import-review-count-models', { count: plan.catalog.models }) : '',
    plan.catalog.groups > 0 ? t('import-review-count-groups', { count: plan.catalog.groups }) : '',
  ].filter(Boolean)
  if (plan.deletable > 0 && made.length > 0) {
    parts.push(t('import-remove-catalog', { list: opts.list(made) }))
  }
  if (opts.webhooks && plan.deletable > 0) {
    parts.push(t('import-remove-webhooks', { count: plan.deletable }))
  }
  return parts.join(' ')
}

const SYNC_SOURCES = new Set(['intune', 'entra'])

/** A kept asset's line: why it stays. */
export function reasonLine(t: Translate, reason: KeptReason, date: (iso: string) => string): string {
  switch (reason.code) {
    case 'edited':
      return reason.by_name
        ? t('import-remove-reason-edited-by', { date: date(reason.at), name: reason.by_name })
        : t('import-remove-reason-edited', { date: date(reason.at) })
    case 'status_changed':
      return t('import-remove-reason-status-changed')
    case 'loan':
      return t('import-remove-reason-loan')
    case 'disposed':
      return t('import-remove-reason-disposed')
    case 'media':
      return reason.photos > 0
        ? t('import-remove-reason-photos', { count: reason.photos })
        : t('import-remove-reason-files', { count: reason.files })
    case 'audited':
      return t('import-remove-reason-audited', { date: date(reason.at) })
    case 'usage':
      return t('import-remove-reason-usage')
    case 'group_added':
      return t('import-remove-reason-group-added')
    case 'ticket':
      return t('import-remove-reason-ticket', { id: String(reason.ticket_id) })
    case 'directory':
      return t('import-remove-reason-directory')
    case 'synced':
      return t(`import-remove-reason-synced-${SYNC_SOURCES.has(reason.source) ? reason.source : 'generic'}`)
    case 'referenced':
      return t('import-remove-reason-referenced', { name: reason.name })
  }
}

/** "398 assets deleted on 15 Oct by Priya Nair. 9 kept." */
export function resultLine(t: Translate, job: ImportJob, date: (iso: string) => string): string {
  const result = job.remove_result
  if (!result || !job.removed_at) return ''
  const when = date(job.removed_at)
  const head = job.removed_by_name
    ? t('import-remove-result', { count: result.deleted, date: when, name: job.removed_by_name })
    : t('import-remove-result-anonymous', { count: result.deleted, date: when })
  return result.kept.length > 0
    ? `${head} ${t('import-remove-result-kept', { count: result.kept.length })}`
    : head
}

/** The previous values help text, with how many fields were blank before
 *  when there were any. */
export function previousValuesHelp(t: Translate, blankBefore: number): string {
  return blankBefore > 0
    ? t('import-previous-help-blank', { count: blankBefore })
    : t('import-previous-help')
}
