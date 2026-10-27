/**
 * Review's numbers and lines for a documentation import, kept out of the
 * components so they can be tested alone.
 *
 * Every page's result is read once and filtered here: the server's filters
 * can't apply a note chip, and "Skip them" or "Import as copies" is a commit
 * option, so flipping it recomputes each outcome, the headline, the tiles
 * and the Import button without a new check. A page the check couldn't
 * convert is still imported, with an empty body; only pages already in
 * Nosdesk can be skipped.
 */
import type {
  DocsDuplicates,
  DocsImageProblem,
  DocsNoteCode,
  DocsPageRow,
  DocsSummary,
} from '@nosdesk/core/services/docsImportService'

type Translate = (key: string, args?: Record<string, string | number>) => string

export type RowOutcome = 'new' | 'copy' | 'skipped' | 'error'
export type ListFilter = 'all' | 'notes' | 'skipped' | 'errors'
/** A note code, or one of the lines that aren't codes. */
export type ListChip = DocsNoteCode | 'sections' | 'images' | 'links' | 'changed' | 'errors'

export const PAGE_SIZE = 25

/** What the commit will do with a page under this duplicates choice, as
 *  the server decides it. */
export function outcomeOf(row: DocsPageRow, choice: DocsDuplicates): RowOutcome {
  if (row.duplicate && choice === 'skip') return 'skipped'
  if (row.error) return 'error'
  if (row.duplicate) return 'copy'
  return 'new'
}

export interface PlanCounts {
  checked: number
  /** Pages the commit writes: new, copies and pages imported empty. */
  imported: number
  copies: number
  skipped: number
  errors: number
  duplicates: number
}

export function planCounts(rows: DocsPageRow[], choice: DocsDuplicates): PlanCounts {
  const c: PlanCounts = { checked: rows.length, imported: 0, copies: 0, skipped: 0, errors: 0, duplicates: 0 }
  for (const row of rows) {
    if (row.duplicate) c.duplicates++
    const o = outcomeOf(row, choice)
    if (o === 'skipped') c.skipped++
    else c.imported++
    if (o === 'copy') c.copies++
    if (o === 'error') c.errors++
  }
  return c
}

/** The counts from the summary alone, for a job whose rows aren't read
 *  (the detail page after the prune). */
export function summaryCounts(summary: DocsSummary): PlanCounts {
  const copies = summary.duplicates_choice === 'copy' ? summary.duplicates : 0
  return {
    checked: summary.row_count,
    imported: summary.would_create,
    copies,
    skipped: summary.skipped,
    errors: summary.errors,
    duplicates: summary.duplicates,
  }
}

/** A section page: it stands for a folder and has no file of its own. */
export function isSection(row: DocsPageRow): boolean {
  return !/\.(md|markdown|mdx|csv)$/i.test(row.path)
}

export function imageProblems(row: DocsPageRow) {
  return (row.images ?? []).filter((i) => i.problem)
}

function keptAsText(row: DocsPageRow): number {
  return (row.links.unresolved?.length ?? 0) + (row.links.attachments?.length ?? 0)
}

/** The server's "with notes": a note code, or an error. */
export function hasNotes(row: DocsPageRow): boolean {
  return Object.keys(row.notes ?? {}).length > 0 || !!row.error
}

export function matchesChip(row: DocsPageRow, chip: ListChip, changed: ReadonlySet<number>): boolean {
  switch (chip) {
    case 'sections':
      return isSection(row)
    case 'images':
      return imageProblems(row).length > 0
    case 'links':
      return keptAsText(row) > 0
    case 'changed':
      return changed.has(row.index)
    case 'errors':
      return !!row.error
    default:
      return (row.notes?.[chip] ?? 0) > 0
  }
}

export function filterRows(
  rows: DocsPageRow[],
  filter: ListFilter,
  chip: ListChip | null,
  choice: DocsDuplicates,
  changed: ReadonlySet<number> = new Set(),
): DocsPageRow[] {
  return rows.filter((row) => {
    const o = outcomeOf(row, choice)
    if (filter === 'notes' && (!hasNotes(row) || o === 'skipped')) return false
    if (filter === 'skipped' && o !== 'skipped') return false
    if (filter === 'errors' && o !== 'error') return false
    return chip ? matchesChip(row, chip, changed) : true
  })
}

export function filterCounts(
  rows: DocsPageRow[],
  choice: DocsDuplicates,
): Record<ListFilter, number> {
  const out: Record<ListFilter, number> = { all: rows.length, notes: 0, skipped: 0, errors: 0 }
  for (const row of rows) {
    const o = outcomeOf(row, choice)
    if (hasNotes(row) && o !== 'skipped') out.notes++
    if (o === 'skipped') out.skipped++
    if (o === 'error') out.errors++
  }
  return out
}

export interface Paged<T> {
  rows: T[]
  page: number
  pages: number
  from: number
  to: number
  total: number
}

export function paginate<T>(rows: T[], page: number, size = PAGE_SIZE): Paged<T> {
  const pages = Math.max(1, Math.ceil(rows.length / size))
  const p = Math.min(Math.max(1, page), pages)
  const start = (p - 1) * size
  const slice = rows.slice(start, start + size)
  return {
    rows: slice,
    page: p,
    pages,
    from: rows.length === 0 ? 0 : start + 1,
    to: start + slice.length,
    total: rows.length,
  }
}

/** Where a page sits: its parents' titles, top first. */
export function crumbs(row: DocsPageRow, byIndex: ReadonlyMap<number, DocsPageRow>): string {
  const out: string[] = []
  const seen = new Set<number>()
  for (let at = row.parent; at != null && !seen.has(at); ) {
    seen.add(at)
    const parent = byIndex.get(at)
    if (!parent) break
    out.unshift(parent.title)
    at = parent.parent
  }
  return out.join(' / ')
}

export const NOTE_KEY: Record<DocsNoteCode, string> = {
  tables_hoisted: 'docs-import-note-tables-hoisted',
  table_rows_padded: 'docs-import-note-table-rows-padded',
  table_cells_flattened: 'docs-import-note-table-cells-flattened',
  checklists_as_bullets: 'docs-import-note-checklists-as-bullets',
  strikethrough_removed: 'docs-import-note-strikethrough-removed',
  callouts_as_quotes: 'docs-import-note-callouts-as-quotes',
  formulas_as_code: 'docs-import-note-formulas-as-code',
  links_dropped: 'docs-import-note-links-dropped',
  images_dropped: 'docs-import-note-images-dropped',
  html_removed: 'docs-import-note-html-removed',
  html_as_text: 'docs-import-note-html-as-text',
  nesting_flattened: 'docs-import-note-nesting-flattened',
  table_cells_dropped: 'docs-import-note-table-cells-dropped',
  mentions_as_text: 'docs-import-note-mentions-as-text',
  ticket_links_as_text: 'docs-import-note-ticket-links-as-text',
  embeds_as_text: 'docs-import-note-embeds-as-text',
  links_unresolved: 'docs-import-note-links-unresolved',
  links_ambiguous: 'docs-import-note-links-ambiguous',
  attachments_left_out: 'docs-import-note-attachments-left-out',
  front_matter_ignored: 'docs-import-note-front-matter-ignored',
  title_shortened: 'docs-import-note-title-shortened',
  not_utf8: 'docs-import-note-not-utf8',
  source_syntax_removed: 'docs-import-note-source-syntax-removed',
  database_rows_cut: 'docs-import-note-database-rows-cut',
  images_unreadable: 'docs-import-note-images-unreadable',
  svg_images_refused: 'docs-import-note-svg-images-refused',
  web_images_not_fetched: 'docs-import-note-web-images-not-fetched',
}

export const IMAGE_PROBLEM_KEY: Record<DocsImageProblem, string> = {
  unreadable: 'docs-import-image-unreadable',
  too_large: 'docs-import-image-too-large',
  not_an_image: 'docs-import-image-not-an-image',
  svg: 'docs-import-image-svg',
  refused_address: 'docs-import-image-refused-address',
  timed_out: 'docs-import-image-timed-out',
  fetch_failed: 'docs-import-image-fetch-failed',
  over_limit: 'docs-import-image-over-limit',
}

/** A note code's line, worded for a count; a code this client doesn't
 *  know reads as the code. */
export function noteText(t: Translate, code: DocsNoteCode, count: number): string {
  const key = NOTE_KEY[code]
  return key ? t(key, { count }) : code
}

export interface NoteLine {
  text: string
  tone?: 'error' | 'warning'
}

/** A page's notes as the list and the preview show them: its outcome, then
 *  each change, naming the image, link or attachment where there is one. */
export function rowNotes(
  t: Translate,
  row: DocsPageRow,
  choice: DocsDuplicates,
  changed: ReadonlySet<number> = new Set(),
): NoteLine[] {
  const out: NoteLine[] = []
  const o = outcomeOf(row, choice)
  if (row.error) out.push({ text: row.error, tone: 'error' })
  if (o === 'skipped') out.push({ text: t('docs-import-row-already-skip') })
  if (o === 'copy' || (o === 'error' && row.duplicate)) out.push({ text: t('docs-import-row-already-copy') })
  if (changed.has(row.index)) out.push({ text: t('docs-import-row-changed'), tone: 'warning' })
  const problems = imageProblems(row)
  const unresolved = row.links.unresolved ?? []
  const attachments = row.links.attachments ?? []
  const named: Partial<Record<DocsNoteCode, boolean>> = {
    links_unresolved: unresolved.length > 0,
    attachments_left_out: attachments.length > 0,
    images_unreadable: problems.length > 0,
    svg_images_refused: problems.length > 0,
    web_images_not_fetched: problems.length > 0,
  }
  for (const [code, count] of Object.entries(row.notes ?? {}) as [DocsNoteCode, number][]) {
    if (!named[code]) out.push({ text: noteText(t, code, count) })
  }
  for (const image of problems) {
    out.push({
      text: t('docs-import-row-image-problem', {
        name: image.name,
        problem: t(IMAGE_PROBLEM_KEY[image.problem!] ?? 'docs-import-image-unreadable'),
      }),
    })
  }
  for (const name of attachments) out.push({ text: t('docs-import-row-attachment', { name }) })
  for (const name of unresolved) out.push({ text: t('docs-import-row-unresolved', { name }) })
  if (isSection(row)) out.push({ text: t('docs-import-row-section') })
  return out
}

/** The destination as a sentence part: "into a new collection, IT
 *  Handbook", "into IT Handbook under Network", "into 6 new collections". */
export interface DestinationWords {
  mode: 'new' | 'existing' | 'many'
  name: string
  under?: string | null
  count?: number
}

export function destinationText(t: Translate, d: DestinationWords): string {
  switch (d.mode) {
    case 'new':
      return t('docs-import-dest-new', { name: d.name })
    case 'many':
      return t('docs-import-dest-many', { count: d.count ?? 0 })
    default:
      return d.under
        ? t('docs-import-dest-existing-under', { name: d.name, under: d.under })
        : t('docs-import-dest-existing', { name: d.name })
  }
}

/** Review's headline: "39 pages checked. 36 will be imported into IT
 *  Handbook, as drafts. 3 skipped: already in Nosdesk." */
export function headline(
  t: Translate,
  c: PlanCounts,
  dest: string,
  status: 'drafts' | 'published' | 'as_files',
): string {
  const parts = [t('docs-import-headline-checked', { count: c.checked })]
  if (c.imported > 0) {
    parts.push(
      t('docs-import-headline-imported', {
        count: c.imported,
        dest,
        status,
        copies: c.copies,
      }),
    )
  } else {
    parts.push(t('docs-import-headline-nothing'))
  }
  if (c.skipped > 0) parts.push(t('docs-import-headline-skipped', { count: c.skipped }))
  if (c.errors > 0) parts.push(t('docs-import-headline-errors', { count: c.errors }))
  return parts.join(' ')
}

/** The confirmation's message. */
export function confirmText(
  t: Translate,
  c: PlanCounts,
  images: number,
  dest: string,
  status: 'drafts' | 'published' | 'as_files',
): string {
  const parts = [t('docs-import-confirm-message', { count: c.imported, images, dest, status })]
  if (c.skipped > 0) parts.push(t('docs-import-confirm-skipped', { count: c.skipped }))
  if (c.copies > 0) parts.push(t('docs-import-confirm-copies', { count: c.copies }))
  if (c.errors > 0) parts.push(t('docs-import-confirm-errors', { count: c.errors }))
  if (status === 'published') parts.push(t('docs-import-confirm-published'))
  return parts.join(' ')
}

/** Pages whose plan moved between two checks of the same job: found in
 *  Nosdesk now and not before, or the other way. */
export function changedRows(before: DocsPageRow[], after: DocsPageRow[]): Set<number> {
  const was = new Map(before.map((r) => [r.index, r.duplicate?.page_id ?? null]))
  const out = new Set<number>()
  for (const row of after) {
    if (!was.has(row.index)) continue
    if ((was.get(row.index) ?? null) !== (row.duplicate?.page_id ?? null)) out.add(row.index)
  }
  return out
}

/** The collection Open goes to after the import: the existing one it went
 *  into, or the new one by its name (the newest of that name). `null` when
 *  the pages went into several collections, or the new one hasn't arrived
 *  yet. */
export function collectionAfterImport<C extends { id: number; name: string }>(
  summary: Pick<DocsSummary, 'destination'> | null | undefined,
  collections: readonly C[],
): C | null {
  const d = summary?.destination
  if (!d) return null
  if (d.mode === 'existing' && d.collection_id != null) {
    return collections.find((c) => c.id === d.collection_id) ?? null
  }
  if (d.new_collections.length !== 1) return null
  const name = d.new_collections[0].toLowerCase()
  return [...collections].filter((c) => c.name.toLowerCase() === name).sort((a, b) => b.id - a.id)[0] ?? null
}
