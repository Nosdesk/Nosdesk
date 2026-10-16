import { describe, expect, it } from 'vitest'
import type {
  ImportJob,
  ImportJobListItem,
  ImportSummary,
} from '@nosdesk/core/services/importService'

import {
  STATE_TONE,
  countsLine,
  doneHeadline,
  fileLabel,
  optionsLine,
  reportOffered,
  rowAction,
} from '../historyModel'

/** Echoes the key and its arguments, so the words chosen are visible. */
const t = (key: string, args?: Record<string, string | number>) =>
  args ? `${key}(${Object.entries(args).map(([k, v]) => `${k}=${v}`).join(',')})` : key

function item(overrides: Partial<ImportJobListItem> = {}): ImportJobListItem {
  return {
    id: 'job-1',
    job_type: 'assets',
    status: 'done',
    state: 'imported',
    filename: 'Cameras.xlsx',
    sheet: null,
    created_by: 'u-1',
    created_by_name: 'Priya Nair',
    created_at: '2026-10-15T00:00:00Z',
    updated_at: '2026-10-15T00:00:00Z',
    completed_at: '2026-10-15T00:00:00Z',
    pruned_at: null,
    counts: { new: 212, updated: 168, unchanged: 0, skipped: 32 },
    ...overrides,
  }
}

function summary(overrides: Partial<ImportSummary> = {}): ImportSummary {
  return {
    row_count: 412,
    would_create: 407,
    would_update: 0,
    unchanged: 0,
    errors: [],
    errors_truncated: false,
    errors_total: 5,
    rows_with_errors: 5,
    ...overrides,
  }
}

function job(overrides: Partial<ImportJob> = {}): ImportJob {
  return {
    id: 'job-1',
    job_type: 'assets',
    status: 'done',
    state: 'imported',
    filename: 'Cameras.xlsx',
    created_by: null,
    created_at: '2026-10-15T00:00:00Z',
    updated_at: '2026-10-15T00:00:00Z',
    completed_at: '2026-10-15T00:00:00Z',
    summary: summary(),
    records_committed: 407,
    error_message: null,
    source: null,
    suggested_mapping: [],
    mapping: null,
    options: null,
    ...overrides,
  }
}

describe('Recent imports rows', () => {
  it('pills Draft as caution, Imported as positive and Expired as neutral', () => {
    expect(STATE_TONE).toEqual({ draft: 'caution', imported: 'positive', expired: 'neutral' })
  })

  it('resumes a draft, views an import, and offers nothing on an expired draft', () => {
    expect(rowAction({ state: 'draft' })).toBe('resume')
    expect(rowAction({ state: 'imported' })).toBe('view')
    expect(rowAction({ state: 'expired' })).toBeNull()
  })

  it('counts what an import did, leaving out the zeros', () => {
    expect(countsLine(t, item())).toBe(
      'import-history-count-new(count=212) · import-history-count-updated(count=168) · import-history-count-skipped(count=32)',
    )
    expect(countsLine(t, item({ counts: { new: 0, updated: 0, unchanged: 0, skipped: 0 } }))).toBe(
      'import-history-count-none',
    )
  })

  it('says how many created assets were removed, once removal ran', () => {
    expect(countsLine(t, item({ removed: { deleted: 398, created: 407 } }))).toBe(
      'import-history-count-removed(deleted=398,created=407)',
    )
  })

  it('says where a draft stopped, and that an expired one expired', () => {
    expect(countsLine(t, item({ state: 'draft', status: 'parsed', counts: null }))).toBe('import-history-draft-on-map')
    expect(countsLine(t, item({ state: 'draft', status: 'dry_run_done' }))).toBe('import-history-draft-on-review')
    expect(countsLine(t, item({ state: 'expired', status: 'dry_run_done' }))).toBe('import-history-state-expired')
  })

  it('names a workbook with its sheet', () => {
    expect(fileLabel({ filename: 'Inventory.xlsx', sheet: 'Cameras' })).toBe('Inventory.xlsx · Cameras')
    expect(fileLabel({ filename: 'termly.csv', sheet: null })).toBe('termly.csv')
  })
})

describe('Done', () => {
  it('reads the headline from the counts', () => {
    expect(doneHeadline(t, job())).toBe(
      'import-done-headline(count=407,parts=import-done-created(type=assets,count=407)) import-review-skipped(count=5)',
    )
    expect(
      doneHeadline(
        t,
        job({ summary: summary({ row_count: 180, would_create: 30, would_update: 150, rows_with_errors: 0 }) }),
      ),
    ).toBe('import-done-headline(count=180,parts=import-done-created(type=assets,count=30), import-done-updated(count=150))')
    expect(
      doneHeadline(t, job({ summary: summary({ would_create: 0, unchanged: 407, rows_with_errors: 5 }) })),
    ).toBe('import-done-nothing import-review-unchanged(count=407) import-review-skipped(count=5)')
  })

  it('offers the error report while the detail is kept and it has rows', () => {
    expect(reportOffered(job())).toBe(true)
    expect(reportOffered(job({ pruned_at: '2026-11-15T00:00:00Z' }))).toBe(false)
    expect(reportOffered(job({ summary: summary({ rows_with_errors: 0 }) }))).toBe(false)
  })

  it('lists the options as Map words them', () => {
    expect(
      optionsLine(t, 'assets', {
        match_by: 'serial_number',
        mode: 'create_only',
        date_order: 'dmy',
        decimal: ',',
        constants: [{ field: 'kind', value: 'camera' }],
      }),
    ).toBe(
      'import-map-summary-match-assets(key=serial_number) · import-options-mode-create-only · import-map-summary-dates(order=dmy) · import-map-summary-decimal(mark=comma) · import-history-options-constants(count=1)',
    )
    expect(optionsLine(t, 'users', null)).toBe('import-map-summary-match-users')
  })
})
