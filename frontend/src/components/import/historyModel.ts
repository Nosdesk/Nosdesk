/**
 * Import history rules, kept out of the components so they can be tested
 * on their own: the state pill's tone, the counts line Recent imports
 * shows, the Done headline, and whether a past import still has its
 * detail and its error report.
 *
 * Expired is the server's word (`state`): a draft untouched for 30 days or
 * one whose file was removed. A committed job stays Imported; once its
 * detail is removed (`pruned_at`) it is counts, options and mapping only.
 */
import type {
  ImportJob,
  ImportJobListItem,
  ImportJobState,
  ImportJobType,
  ImportOptions,
} from '@nosdesk/core/services/importService'

import type { StatusPillTone } from '@/components/common/statusPillTone'

import { outcomes, reportHasRows } from './reviewModel'

type Translate = (key: string, args?: Record<string, string | number>) => string

/** How many jobs Recent imports lists. */
export const RECENT_LIMIT = 20

export const STATE_TONE: Record<ImportJobState, StatusPillTone> = {
  draft: 'caution',
  imported: 'positive',
  expired: 'neutral',
}

export const STATE_KEY: Record<ImportJobState, string> = {
  draft: 'import-history-state-draft',
  imported: 'import-history-state-imported',
  expired: 'import-history-state-expired',
}

/** The row's action: a draft resumes, an import opens its detail, an
 *  expired draft has none. */
export function rowAction(item: Pick<ImportJobListItem, 'state'>): 'resume' | 'view' | null {
  switch (item.state) {
    case 'draft':
      return 'resume'
    case 'imported':
      return 'view'
    default:
      return null
  }
}

/** The counts line: where a draft stopped, or what an import did. */
export function countsLine(t: Translate, item: ImportJobListItem): string {
  if (item.state === 'expired') return t('import-history-state-expired')
  if (item.state === 'draft') {
    return item.status === 'parsed' ? t('import-history-draft-on-map') : t('import-history-draft-on-review')
  }
  const c = item.counts
  if (!c) return ''
  const bits: string[] = []
  if (c.new > 0) bits.push(t('import-history-count-new', { count: c.new }))
  if (c.updated > 0) bits.push(t('import-history-count-updated', { count: c.updated }))
  if (c.unchanged > 0) bits.push(t('import-history-count-unchanged', { count: c.unchanged }))
  if (c.skipped > 0) bits.push(t('import-history-count-skipped', { count: c.skipped }))
  return bits.length > 0 ? bits.join(' · ') : t('import-history-count-none')
}

/** The file as Recent imports names it: with the sheet for a workbook. */
export function fileLabel(item: { filename: string; sheet?: string | null }): string {
  return item.sheet ? `${item.filename} · ${item.sheet}` : item.filename
}

/** Done's headline: "407 rows imported: 407 assets created. 5 skipped." */
export function doneHeadline(t: Translate, job: ImportJob): string {
  if (!job.summary) return t('import-done-heading')
  const o = outcomes(job.summary)
  const parts: string[] = []
  if (o.imported === 0) {
    parts.push(t('import-done-nothing'))
  } else {
    const bits: string[] = []
    if (o.created > 0) bits.push(t('import-done-created', { type: job.job_type, count: o.created }))
    if (o.updated > 0) bits.push(t('import-done-updated', { count: o.updated }))
    parts.push(t('import-done-headline', { count: o.imported, parts: bits.join(', ') }))
  }
  if (o.unchanged > 0) parts.push(t('import-review-unchanged', { count: o.unchanged }))
  if (o.skipped > 0) parts.push(t('import-review-skipped', { count: o.skipped }))
  return parts.join(' ')
}

/** Whether a job's per-row detail is still kept. */
export function detailsKept(job: Pick<ImportJob, 'pruned_at'>): boolean {
  return !job.pruned_at
}

/** Whether Done and the detail offer the error report: it has rows and
 *  the detail it is built from is still kept. */
export function reportOffered(job: ImportJob): boolean {
  if (!job.summary || !detailsKept(job)) return false
  return reportHasRows(outcomes(job.summary).skipped, job.summary)
}

/** The import's options in one line, worded as Map's folded summary:
 *  "Matching by asset tag · Create and update · Dates day first". */
export function optionsLine(t: Translate, type: ImportJobType, options: ImportOptions | null): string {
  const o = options ?? {}
  const parts: string[] = []
  if (type === 'assets') {
    parts.push(t('import-map-summary-match-assets', { key: o.match_by ?? 'asset_tag' }))
    parts.push(t(`import-options-mode-${(o.mode ?? 'create_update').replace('_', '-')}`))
  } else {
    parts.push(t(`import-map-summary-match-${type}`))
  }
  if (o.date_order) parts.push(t('import-map-summary-dates', { order: o.date_order }))
  if (o.decimal === ',') parts.push(t('import-map-summary-decimal', { mark: 'comma' }))
  const constants = o.constants?.length ?? 0
  if (constants > 0) parts.push(t('import-history-options-constants', { count: constants }))
  return parts.join(' · ')
}

/** Where a draft resumes: the import wizard at its step. */
export function resumeTarget(id: string) {
  return { path: '/admin/data-import/csv', query: { job: id } }
}

/** A past import's page. */
export function detailPath(id: string): string {
  return `/admin/data-import/imports/${id}`
}
