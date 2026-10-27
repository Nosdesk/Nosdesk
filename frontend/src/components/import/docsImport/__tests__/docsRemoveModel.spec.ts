import { describe, expect, it } from 'vitest'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import enUS from '../../../../../../i18n/locales/en-US/main.ftl?raw'
import frFR from '../../../../../../i18n/locales/fr-FR/main.ftl?raw'
import nlNL from '../../../../../../i18n/locales/nl-NL/main.ftl?raw'
import type {
  DocsImportJob,
  DocsKeptReason,
  DocsRemovalPlan,
  DocsRemoveResult,
  DocsSummary,
} from '@nosdesk/core/services/docsImportService'

import { docsActionRefusalText } from '../../refusals'
import {
  destinationOf,
  docsRemovalState,
  keptLine,
  nothingReason,
  publishOffered,
  publishedLine,
  reasonLine,
  removeMessage,
  removedState,
  resultCollections,
  resultHeadline,
} from '../docsRemoveModel'

/** `t` over a real catalogue, failing on a missing message or a bad
 *  argument. */
function translator(locale: string, source: string) {
  const bundle = new FluentBundle(locale, { useIsolating: false })
  bundle.addResource(new FluentResource(source))
  return (key: string, args?: Record<string, string | number>) => {
    const message = bundle.getMessage(key)
    if (!message?.value) throw new Error(`${locale} has no ${key}`)
    const errors: Error[] = []
    const out = bundle.formatPattern(message.value, args, errors)
    if (errors.length > 0) throw errors[0]
    return out
  }
}

const t = translator('en-US', enUS)
const date = (iso: string) => (iso.startsWith('2026-10-21') ? '21 Oct' : iso)

const EDITED: DocsKeptReason = { code: 'edited', at: '2026-10-21T09:00:00Z', by: 'u-2', by_name: 'Sam Okafor' }

function plan(over: Partial<DocsRemovalPlan> = {}): DocsRemovalPlan {
  return {
    created: 36,
    deletable: 34,
    gone: 0,
    kept: [
      { id: 2, title: 'Laptop setup', reasons: [EDITED] },
      { id: 12, title: 'Hardware', reasons: [{ code: 'pages_added', count: 1 }] },
    ],
    collections: { deleted: [], kept: [] },
    ...over,
  }
}

function result(over: Partial<DocsRemoveResult> = {}): DocsRemoveResult {
  return {
    kind: 'documentation',
    created: 36,
    deleted: 34,
    gone: 0,
    deleted_ids: [],
    kept: plan().kept,
    collections: { deleted: [], kept: [{ id: 7, name: 'IT Handbook', pages: 2 }] },
    ...over,
  }
}

function job(over: Partial<DocsImportJob> = {}): DocsImportJob {
  return {
    id: 'job-1',
    job_type: 'documentation',
    status: 'done',
    filename: 'IT-Handbook.zip',
    created_by: null,
    created_at: '2026-10-19T00:00:00Z',
    updated_at: '2026-10-19T00:00:00Z',
    completed_at: '2026-10-19T00:00:00Z',
    summary: null,
    records_committed: 36,
    error_message: null,
    source: null,
    options: null,
    removal_closes_at: '2026-11-18T00:00:00Z',
    ...over,
  }
}

const NEW = { existing: false, name: 'IT Handbook', created: 1 }

describe('kept pages', () => {
  it('words every reason a page is kept', () => {
    expect(reasonLine(t, EDITED, date)).toBe('Edited on 21 Oct by Sam Okafor')
    expect(reasonLine(t, { code: 'edited', at: '2026-10-21T09:00:00Z' }, date)).toBe('Edited on 21 Oct')
    expect(reasonLine(t, { code: 'pages_added', count: 1 }, date)).toBe('Has 1 page added since')
    expect(reasonLine(t, { code: 'pages_added', count: 3 }, date)).toBe('Has 3 pages added since')
    expect(reasonLine(t, { code: 'moved' }, date)).toBe('Moved to another collection')
    expect(reasonLine(t, { code: 'kept_pages_under', count: 1 }, date)).toBe('Kept because 1 page under it changed')
    expect(reasonLine(t, { code: 'kept_pages_under', count: 2 }, date)).toBe('Kept because 2 pages under it changed')
  })

  it('shows the first reason of several', () => {
    expect(keptLine(t, { id: 1, title: 'A', reasons: [{ code: 'moved' }, EDITED] }, date)).toBe('Moved to another collection')
    expect(keptLine(t, { id: 1, title: 'A', reasons: [] }, date)).toBe('')
  })

  it('has every reason in French and Dutch too', () => {
    for (const [locale, source] of [
      ['fr-FR', frFR],
      ['nl-NL', nlNL],
    ] as const) {
      const other = translator(locale, source)
      for (const reason of [
        EDITED,
        { code: 'edited', at: '2026-10-21T09:00:00Z' },
        { code: 'pages_added', count: 2 },
        { code: 'moved' },
        { code: 'kept_pages_under', count: 2 },
      ] as DocsKeptReason[]) {
        expect(reasonLine(other, reason, date)).not.toBe('')
      }
      expect(removeMessage(other, plan({ gone: 1 }), NEW, '18 Nov')).not.toBe('')
      expect(resultHeadline(other, result())).not.toBe('')
      expect(publishedLine(other, 34, 2)).not.toBe('')
    }
  })
})

describe('the removal dialog', () => {
  it('says what goes, what stays, the collection and the window', () => {
    expect(removeMessage(t, plan(), NEW, '18 Nov')).toBe(
      'Of the 36 pages this import created, 34 are untouched and go to Trash, where they can be restored. ' +
        '2 have changed since and are kept. The collection is deleted if it ends up empty. ' +
        'This can be done once, until 18 Nov.',
    )
    expect(removeMessage(t, plan({ kept: [], gone: 2 }), { existing: true, name: 'IT Handbook', created: 0 }, '18 Nov')).toBe(
      'Of the 36 pages this import created, 34 are untouched and go to Trash, where they can be restored. ' +
        '2 were already deleted or moved to Trash. IT Handbook keeps its other pages. This can be done once, until 18 Nov.',
    )
    expect(removeMessage(t, plan(), { existing: false, name: 'Onboarding', created: 6 }, '18 Nov')).toContain(
      'Collections that end up empty are deleted.',
    )
  })

  it('says why there is nothing to move', () => {
    expect(nothingReason({ created: 0, gone: 0 })).toBe('none_created')
    expect(nothingReason({ created: 3, gone: 3 })).toBe('all_gone')
    expect(nothingReason({ created: 3, gone: 1 })).toBe('all_changed')
    expect(removeMessage(t, plan({ deletable: 0 }), NEW, '18 Nov')).toBe(
      'Every page this import created has been changed since, so none can be moved to Trash.',
    )
  })
})

describe('the removal result', () => {
  it('heads with what moved and what was kept', () => {
    expect(resultHeadline(t, result())).toBe('34 pages moved to Trash. 2 kept.')
  })

  it('says what became of the collections', () => {
    expect(resultCollections(t, result(), NEW)).toBe('IT Handbook was kept: 2 pages remain in it.')
    expect(
      resultCollections(t, result({ kept: [], collections: { deleted: [{ id: 7, name: 'IT Handbook', pages: 0 }], kept: [] } }), NEW),
    ).toBe('IT Handbook was deleted because it ended up empty.')
    expect(resultCollections(t, result({ collections: { deleted: [], kept: [] } }), { existing: true, name: 'IT', created: 0 })).toBe(
      'IT was kept: it still has its other pages.',
    )
    const many = {
      deleted: [
        { id: 1, name: 'A', pages: 0 },
        { id: 2, name: 'B', pages: 0 },
      ],
      kept: [{ id: 3, name: 'C', pages: 4 }],
    }
    expect(resultCollections(t, result({ collections: many }), { existing: false, name: 'A', created: 3 })).toBe(
      '2 collections that ended up empty were deleted.',
    )
    expect(
      resultCollections(t, result({ collections: { deleted: [], kept: many.deleted } }), { existing: false, name: 'A', created: 2 }),
    ).toBe('The collections were kept: they still have pages.')
  })

  it('states a removed import on its page', () => {
    expect(removedState(t, job({ remove_result: result(), removed_at: '2026-10-21T10:00:00Z' }), date)).toBe(
      'Removed: 34 pages moved to Trash on 21 Oct, 2 kept',
    )
  })
})

describe('when removal and Publish all are offered', () => {
  const now = Date.parse('2026-10-20T00:00:00Z')
  it('offers removal for 30 days after a commit that wrote pages, then says it closed', () => {
    expect(docsRemovalState(job(), now)).toBe('offer')
    expect(docsRemovalState(job(), Date.parse('2026-11-19T00:00:00Z'))).toBe('closed')
    expect(docsRemovalState(job({ remove_result: result(), removed_at: '2026-10-21T00:00:00Z' }), now)).toBe('removed')
    expect(docsRemovalState(job({ records_committed: 0 }), now)).toBeNull()
    expect(docsRemovalState(job({ status: 'committing' }), now)).toBeNull()
  })

  it('offers Publish all unless the pages were published or removed', () => {
    const summary = (status: DocsSummary['status']) => ({ status }) as DocsSummary
    expect(publishOffered(job(), summary('drafts'))).toBe(true)
    expect(publishOffered(job(), summary('as_files'))).toBe(true)
    expect(publishOffered(job(), summary('published'))).toBe(false)
    expect(publishOffered(job({ removed_at: '2026-10-21T00:00:00Z' }), summary('drafts'))).toBe(false)
    expect(publishOffered(job({ status: 'committing' }), summary('drafts'))).toBe(false)
  })

  it('words what Publish all did', () => {
    expect(publishedLine(t, 34, 0)).toBe('34 pages published')
    expect(publishedLine(t, 34, 1)).toBe('34 pages published · 1 left as it was (changed since the import)')
    expect(publishedLine(t, 33, 2)).toBe('33 pages published · 2 left as they were (changed since the import)')
  })

  it('reads where the pages went', () => {
    const d = (destination: Partial<DocsSummary['destination']>) =>
      ({ destination: { mode: 'new', new_collections: [], collection_id: null, collection_name: null, under_page_id: null, ...destination } }) as DocsSummary
    expect(destinationOf(d({ mode: 'existing', collection_name: 'IT' }))).toEqual({ existing: true, name: 'IT', created: 0 })
    expect(destinationOf(d({ new_collections: ['IT Handbook'] }))).toEqual({ existing: false, name: 'IT Handbook', created: 1 })
    expect(destinationOf(null)).toEqual({ existing: false, name: '', created: 0 })
  })
})

describe('refusals', () => {
  it('words every refusal of Remove imported pages and Publish all', () => {
    const body = (code: string, args?: Record<string, string>) => ({ code, error: 'server', args })
    expect(docsActionRefusalText(t, body('import_not_committed'))).toBe('This import wasn\'t committed, so it created no pages.')
    expect(docsActionRefusalText(t, body('import_already_removed'))).toBe(
      'The pages this import created were already moved to Trash.',
    )
    expect(docsActionRefusalText(t, body('import_removal_window_closed'), '18 Nov')).toBe('Removal window closed 18 Nov.')
    expect(docsActionRefusalText(t, body('import_busy'))).toBe('This import is being changed. Try again in a moment.')
    expect(docsActionRefusalText(t, body('import_type_busy'))).toBe(
      'Another documentation import is being committed. Try again when it finishes.',
    )
    expect(docsActionRefusalText(t, body('import_nothing_to_remove', { reason: 'none_created' }))).toBe(
      'This import created no pages, so there is nothing to remove.',
    )
    expect(docsActionRefusalText(t, body('import_nothing_to_remove', { reason: 'all_gone' }))).toBe(
      'The pages this import created were already deleted or moved to Trash.',
    )
    expect(docsActionRefusalText(t, body('import_nothing_to_remove', { reason: 'all_changed' }))).toBe(
      'Every page this import created has been changed since, so none can be moved to Trash.',
    )
    expect(docsActionRefusalText(t, body('import_nothing_to_remove', { reason: 'other' }))).toBe('server')
    expect(docsActionRefusalText(t, body('something_else'))).toBeNull()
    expect(docsActionRefusalText(t, undefined)).toBeNull()
  })
})
