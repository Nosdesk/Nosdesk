import { describe, expect, it } from 'vitest'
import type { DocsPageRow, DocsSummary } from '@nosdesk/core/services/docsImportService'

import {
  changedRows,
  collectionAfterImport,
  confirmText,
  crumbs,
  filterCounts,
  filterRows,
  headline,
  outcomeOf,
  paginate,
  planCounts,
  rowNotes,
  summaryCounts,
} from '../docsReviewModel'

// Translations as their id and arguments, so assertions read the choice.
const t = (key: string, args?: Record<string, string | number>) =>
  args ? `${key}(${Object.entries(args).map(([k, v]) => `${k}=${v}`).join(',')})` : key

function row(index: number, over: Partial<DocsPageRow> = {}): DocsPageRow {
  return {
    index,
    path: `Docs/Page ${index}.md`,
    title: `Page ${index}`,
    parent: null,
    outcome: 'new',
    notes: {},
    links: {},
    ...over,
  }
}

const ROWS: DocsPageRow[] = [
  row(0, { path: 'Docs', title: 'Docs' }),
  row(1, { parent: 0, notes: { callouts_as_quotes: 2 } }),
  row(2, { parent: 0, duplicate: { page_id: 9, page_uuid: 'u', by: 'source' }, outcome: 'skipped' }),
  row(3, { parent: 1, error: 'Too big', outcome: 'error' }),
  row(4, { parent: 0, images: [{ source: 'a.png', kind: 'archive', name: 'a.png', size: 1, problem: 'unreadable' }], notes: { images_unreadable: 1 } }),
  row(5, { parent: 0, links: { unresolved: ['Missing page'] }, notes: { links_unresolved: 1 } }),
]

describe('outcomes', () => {
  it('follow the duplicates choice: skipped, or a copy; a page that failed is still imported', () => {
    expect(outcomeOf(ROWS[2], 'skip')).toBe('skipped')
    expect(outcomeOf(ROWS[2], 'copy')).toBe('copy')
    expect(outcomeOf(ROWS[3], 'skip')).toBe('error')
    expect(planCounts(ROWS, 'skip')).toEqual({ checked: 6, imported: 5, copies: 0, skipped: 1, errors: 1, duplicates: 1 })
    expect(planCounts(ROWS, 'copy')).toEqual({ checked: 6, imported: 6, copies: 1, skipped: 0, errors: 1, duplicates: 1 })
  })

  it('reads the counts from a summary when the rows are gone', () => {
    const summary = { row_count: 6, would_create: 6, skipped: 0, errors: 1, duplicates: 1, duplicates_choice: 'copy' } as DocsSummary
    expect(summaryCounts(summary)).toMatchObject({ imported: 6, copies: 1, skipped: 0 })
  })
})

describe('the page list', () => {
  it('filters by outcome and by a note chip, with counts that follow the choice', () => {
    expect(filterRows(ROWS, 'skipped', null, 'skip').map((r) => r.index)).toEqual([2])
    expect(filterRows(ROWS, 'skipped', null, 'copy')).toEqual([])
    expect(filterRows(ROWS, 'errors', null, 'skip').map((r) => r.index)).toEqual([3])
    expect(filterRows(ROWS, 'all', 'callouts_as_quotes', 'skip').map((r) => r.index)).toEqual([1])
    expect(filterRows(ROWS, 'all', 'sections', 'skip').map((r) => r.index)).toEqual([0])
    expect(filterRows(ROWS, 'all', 'images', 'skip').map((r) => r.index)).toEqual([4])
    expect(filterRows(ROWS, 'all', 'links', 'skip').map((r) => r.index)).toEqual([5])
    expect(filterRows(ROWS, 'all', 'changed', 'skip', new Set([4])).map((r) => r.index)).toEqual([4])
    expect(filterCounts(ROWS, 'skip')).toEqual({ all: 6, notes: 4, skipped: 1, errors: 1 })
  })

  it('pages 25 at a time', () => {
    const many = Array.from({ length: 60 }, (_, i) => i)
    expect(paginate(many, 3)).toMatchObject({ page: 3, pages: 3, from: 51, to: 60, total: 60 })
    expect(paginate(many, 9).page).toBe(3)
    expect(paginate([], 1)).toMatchObject({ from: 0, to: 0, pages: 1 })
  })

  it('names where a page sits', () => {
    const byIndex = new Map(ROWS.map((r) => [r.index, r]))
    expect(crumbs(ROWS[3], byIndex)).toBe('Docs / Page 1')
    expect(crumbs(ROWS[0], byIndex)).toBe('')
  })

  it("lists a page's notes, naming its image, link and outcome", () => {
    expect(rowNotes(t, ROWS[4], 'skip').map((n) => n.text)).toEqual([
      'docs-import-row-image-problem(name=a.png,problem=docs-import-image-unreadable)',
    ])
    expect(rowNotes(t, ROWS[5], 'skip').map((n) => n.text)).toEqual(['docs-import-row-unresolved(name=Missing page)'])
    expect(rowNotes(t, ROWS[3], 'skip')[0]).toEqual({ text: 'Too big', tone: 'error' })
    expect(rowNotes(t, ROWS[2], 'copy').map((n) => n.text)).toEqual(['docs-import-row-already-copy'])
    expect(rowNotes(t, ROWS[1], 'skip', new Set([1]))[0]).toEqual({ text: 'docs-import-row-changed', tone: 'warning' })
  })
})

describe('the headline and confirmation', () => {
  it('repeat the numbers and the destination', () => {
    const c = planCounts(ROWS, 'skip')
    expect(headline(t, c, 'into IT', 'drafts')).toBe(
      'docs-import-headline-checked(count=6) docs-import-headline-imported(count=5,dest=into IT,status=drafts,copies=0) docs-import-headline-skipped(count=1) docs-import-headline-errors(count=1)',
    )
    expect(confirmText(t, c, 3, 'into IT', 'published')).toContain('docs-import-confirm-published')
    expect(headline(t, { ...c, imported: 0 }, 'into IT', 'drafts')).toContain('docs-import-headline-nothing')
  })
})

describe('a changed plan', () => {
  it('marks the pages whose duplicate moved', () => {
    const after = ROWS.map((r) => (r.index === 4 ? { ...r, duplicate: { page_id: 3, page_uuid: 'x', by: 'source' as const } } : r))
    expect([...changedRows(ROWS, after)]).toEqual([4])
  })
})

describe('collectionAfterImport', () => {
  const cols = [
    { id: 1, name: 'IT Handbook' },
    { id: 5, name: 'it handbook' },
    { id: 7, name: 'Policies' },
  ]
  it('finds the existing collection, or the newest of the new name', () => {
    const d = (over: object) => ({ destination: { mode: 'new', new_collections: ['IT Handbook'], collection_id: null, collection_name: null, under_page_id: null, ...over } }) as DocsSummary
    expect(collectionAfterImport(d({ mode: 'existing', collection_id: 7 }), cols)?.id).toBe(7)
    expect(collectionAfterImport(d({}), cols)?.id).toBe(5)
    expect(collectionAfterImport(d({ new_collections: ['A', 'B'] }), cols)).toBeNull()
  })
})
