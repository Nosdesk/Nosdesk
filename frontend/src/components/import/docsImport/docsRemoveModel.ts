/**
 * Remove imported pages and Publish all for a documentation import, kept
 * out of the components so they can be tested on their own: whether a job
 * offers them, the dialogs' sentences, each kept page's reason, and the
 * result lines.
 *
 * Removal moves the pages nobody has changed since to Trash, once, until 30
 * days after the commit (`removal_closes_at`). Publish all publishes the
 * drafts nobody has changed since; it isn't offered when the import wrote
 * published pages, or once its pages were removed.
 */
import type {
  DocsImportJob,
  DocsKeptPage,
  DocsKeptReason,
  DocsRemovalPlan,
  DocsRemoveResult,
  DocsSummary,
} from '@nosdesk/core/services/docsImportService'

type Translate = (key: string, args?: Record<string, string | number>) => string

/** What a committed documentation import shows for removal. */
export type DocsRemovalState = 'offer' | 'removed' | 'closed'

export function docsRemovalState(job: DocsImportJob, now: number = Date.now()): DocsRemovalState | null {
  if (job.status !== 'done') return null
  if (job.removed_at || job.remove_result) return 'removed'
  if ((job.records_committed ?? 0) === 0 || !job.removal_closes_at) return null
  if (Date.parse(job.removal_closes_at) <= now) return 'closed'
  return 'offer'
}

/** Whether Publish all is offered: a done import of drafts (or pages as
 *  the files said), not removed. */
export function publishOffered(job: DocsImportJob, summary: DocsSummary | null): boolean {
  if (job.status !== 'done' || !summary) return false
  if (summary.status === 'published') return false
  return !job.removed_at && !job.remove_result
}

/** A kept page's line: why it stays. */
export function reasonLine(t: Translate, reason: DocsKeptReason, date: (iso: string) => string): string {
  switch (reason.code) {
    case 'edited':
      return reason.by_name
        ? t('docs-import-remove-reason-edited-by', { date: date(reason.at), name: reason.by_name })
        : t('docs-import-remove-reason-edited', { date: date(reason.at) })
    case 'pages_added':
      return t('docs-import-remove-reason-pages-added', { count: reason.count })
    case 'moved':
      return t('docs-import-remove-reason-moved')
    case 'kept_pages_under':
      return t('docs-import-remove-reason-kept-under', { count: reason.count })
  }
}

/** The first reason's line, which the page shows. */
export function keptLine(t: Translate, page: DocsKeptPage, date: (iso: string) => string): string {
  return page.reasons[0] ? reasonLine(t, page.reasons[0], date) : ''
}

/** Where the pages went, for the collection sentences. */
export interface Destination {
  /** Into a collection that was there before. */
  existing: boolean
  /** That collection's name, or the one new collection's. */
  name: string
  /** New collections the import created. */
  created: number
}

export function destinationOf(summary: DocsSummary | null): Destination {
  const d = summary?.destination
  if (!d) return { existing: false, name: '', created: 0 }
  if (d.mode === 'existing') return { existing: true, name: d.collection_name ?? '', created: 0 }
  return { existing: false, name: d.new_collections[0] ?? '', created: d.new_collections.length }
}

/** Why a removal would move nothing: none created, all gone, or every
 *  page changed since. */
export function nothingReason(plan: Pick<DocsRemovalPlan, 'created' | 'gone'>): 'none_created' | 'all_gone' | 'all_changed' {
  if (plan.created === 0) return 'none_created'
  if (plan.gone === plan.created) return 'all_gone'
  return 'all_changed'
}

/** The confirmation's body: what the import created, what goes to Trash
 *  and what stays, what becomes of the collection, and the window. */
export function removeMessage(t: Translate, plan: DocsRemovalPlan, dest: Destination, closes: string): string {
  if (plan.deletable === 0) return t(`docs-import-remove-nothing-${nothingReason(plan).replace(/_/g, '-')}`)
  const parts = [t('docs-import-remove-untouched', { created: plan.created, count: plan.deletable })]
  if (plan.kept.length > 0) parts.push(t('docs-import-remove-changed', { count: plan.kept.length }))
  if (plan.gone > 0) parts.push(t('docs-import-remove-gone', { count: plan.gone }))
  if (dest.existing) parts.push(t('docs-import-remove-existing-keeps', { name: dest.name }))
  else if (dest.created > 1) parts.push(t('docs-import-remove-collections-if-empty'))
  else parts.push(t('docs-import-remove-collection-if-empty'))
  if (closes) parts.push(t('docs-import-remove-once', { date: closes }))
  return parts.join(' ')
}

/** "34 pages moved to Trash. 2 kept." */
export function resultHeadline(t: Translate, result: DocsRemoveResult): string {
  return t('docs-import-remove-result', { count: result.deleted, kept: result.kept.length })
}

/** What became of the collections, after "Restore any of them from Trash
 *  as usual." */
export function resultCollections(t: Translate, result: DocsRemoveResult, dest: Destination): string {
  const { deleted, kept } = result.collections
  if (dest.existing || deleted.length + kept.length === 0) {
    return t('docs-import-remove-result-existing', { name: dest.name })
  }
  if (deleted.length + kept.length === 1) {
    return deleted.length === 1
      ? t('docs-import-remove-result-collection-deleted', { name: deleted[0].name })
      : t('docs-import-remove-result-collection-kept', { name: kept[0].name, count: kept[0].pages })
  }
  return deleted.length > 0
    ? t('docs-import-remove-result-collections-deleted', { count: deleted.length })
    : t('docs-import-remove-result-collections-kept')
}

/** "34 pages published · 2 left as they were (changed since the import)". */
export function publishedLine(t: Translate, published: number, left: number): string {
  const head = t('docs-import-published', { count: published })
  return left > 0 ? `${head} · ${t('docs-import-published-left', { count: left })}` : head
}

/** The state line on the detail page once removal ran. */
export function removedState(t: Translate, job: DocsImportJob, date: (iso: string) => string): string {
  const r = job.remove_result
  if (!r) return ''
  return t('docs-import-detail-state-removed', {
    count: r.deleted,
    date: job.removed_at ? date(job.removed_at) : '',
    kept: r.kept.length,
  })
}
