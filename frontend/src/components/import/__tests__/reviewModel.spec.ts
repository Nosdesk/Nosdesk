import { describe, expect, it } from 'vitest'
import type { ImportField, ImportJob, ImportSummary } from '@nosdesk/core/services/importService'

import {
  defaultFilter,
  moveHighlight,
  notImported,
  outcomes,
  planDelta,
  problems,
  typingIn,
  webhookHearsAssets,
} from '../reviewModel'

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

const FIELDS: ImportField[] = [
  { id: 'name', label: 'Name', group: 'core', value_type: 'text', required: true },
  { id: 'rego', label: 'Rego', group: 'attribute', value_type: 'text', required: false },
  { id: 'groups', label: 'Groups', group: 'links', value_type: 'array', required: false },
  { id: 'primary_user', label: 'Primary user', group: 'links', value_type: 'user', required: false },
]

function job(s: ImportSummary, overrides: Partial<ImportJob> = {}): ImportJob {
  const names = ['Asset #', 'Device Name', 'Room', 'Comments', 'Rego', 'Assigned To', 'Group', 'Owner']
  return {
    id: 'job-1',
    job_type: 'assets',
    status: 'dry_run_done',
    filename: 'cameras.xlsx',
    created_by: null,
    created_at: '',
    updated_at: '',
    completed_at: null,
    summary: s,
    records_committed: null,
    error_message: null,
    source: {
      format: 'xlsx',
      sheets: [],
      sheet: 'Cameras',
      header_row: 3,
      headers: names.map((name, index) => ({ index, name, samples: [] })),
      row_count: 412,
    },
    suggested_mapping: [],
    mapping: { asset_tag: 0, name: 1, rego: 4, primary_user: 5, groups: 6 },
    ignored: [3],
    options: null,
    ...overrides,
  }
}

describe('outcomes', () => {
  it('counts Priya\'s cameras: 407 new and 5 skipped, 407 to import', () => {
    expect(outcomes(summary())).toEqual({
      total: 412,
      created: 407,
      updated: 0,
      unchanged: 0,
      skipped: 5,
      imported: 407,
    })
  })

  it('keeps unchanged rows out of what Import writes', () => {
    const o = outcomes(summary({ row_count: 8, would_create: 7, unchanged: 1, rows_with_errors: 0 }))
    expect(o.imported).toBe(7)
    expect(o.unchanged).toBe(1)
  })
})

describe('defaultFilter', () => {
  it('opens on Skipped, else Updated, else All', () => {
    expect(defaultFilter(outcomes(summary()))).toBe('skipped')
    expect(
      defaultFilter(
        outcomes(summary({ row_count: 180, would_create: 30, would_update: 150, rows_with_errors: 0 })),
      ),
    ).toBe('update')
    expect(defaultFilter(outcomes(summary({ rows_with_errors: 0, row_count: 407 })))).toBe('all')
  })
})

describe('problems', () => {
  it('keeps each group\'s index and fix, and downloads an unclassified one', () => {
    const list = problems(
      summary({
        error_groups: [
          { column: 'kind', message: "'tripod' isn't a kind", count: 9, fix: 'match_values' },
          { column: 'name', message: 'name is required', count: 3 },
        ],
      }),
    )
    expect(list.map((p) => [p.index, p.fix])).toEqual([
      [0, 'match_values'],
      [1, 'download'],
    ])
  })
})

describe('notImported', () => {
  const label = (f: ImportField) => f.label

  it('says everything lands when nothing is left out', () => {
    const result = notImported(job(summary({ unmapped: [], dropped: [] }), { ignored: [] }), FIELDS, label)
    expect(result).toEqual({ lines: [], columns: 0 })
  })

  it('lists columns, attributes, groups and people in source order', () => {
    const s = summary({
      unmapped: [
        { index: 2, values: 380 },
        { index: 3, values: 40 },
        { index: 7, values: 12 },
      ],
      dropped: [
        { column: 'primary_user', source_column: 'Assigned To', kind: 'camera', value: 'a@x.co', count: 2 },
        { column: 'rego', source_column: 'Rego', kind: 'camera', count: 3 },
        { column: 'groups', source_column: 'Group', kind: 'camera', value: 'Field kit 3', count: 8 },
        { column: 'groups', source_column: 'Group', kind: 'vehicle', value: 'Field kit 3', count: 4 },
        { column: 'primary_user', source_column: 'Assigned To', kind: 'vehicle', value: 'A@x.co', count: 1 },
        { column: 'primary_user', source_column: 'Assigned To', kind: 'camera', value: 'b@x.co', count: 2 },
      ],
    })
    const { lines, columns } = notImported(job(s), FIELDS, label)
    expect(lines.map((l) => l.kind)).toEqual(['unmapped', 'ignored', 'attribute', 'person', 'group'])
    expect(lines[0]).toMatchObject({ columns: [2, 7], headers: ['Room', 'Owner'] })
    expect(lines[1]).toMatchObject({ header: 'Comments', values: 40 })
    expect(lines[2]).toMatchObject({ header: 'Rego', kindSlug: 'camera', rows: 3, dropped: [1] })
    // Two emails (case aside) on five rows, every drop of the column.
    expect(lines[3]).toMatchObject({ header: 'Assigned To', values: 2, rows: 5, dropped: [0, 4, 5] })
    // One line per group name, whatever the rows' kinds.
    expect(lines[4]).toMatchObject({ value: 'Field kit 3', rows: 12, dropped: [2, 3] })
    // Room, Comments, Owner, Rego, Assigned To, Group.
    expect(columns).toBe(6)
  })
})

describe('planDelta', () => {
  it('counts the rows the fresh plan skips beyond the reviewed one', () => {
    expect(planDelta(summary({ rows_with_errors: 5 }), summary({ rows_with_errors: 8 }))).toBe(3)
    expect(planDelta(summary({ rows_with_errors: 5 }), summary({ rows_with_errors: 4 }))).toBe(-1)
    expect(planDelta(null, summary())).toBe(0)
  })
})

describe('webhookHearsAssets', () => {
  it('needs an enabled webhook on an asset event', () => {
    expect(webhookHearsAssets([{ enabled: true, events: ['asset.updated'] }])).toBe(true)
    expect(webhookHearsAssets([{ enabled: false, events: ['asset.created'] }])).toBe(false)
    expect(webhookHearsAssets([{ enabled: true, events: ['ticket.created'] }])).toBe(false)
    expect(webhookHearsAssets([])).toBe(false)
  })
})

describe('keyboard', () => {
  it('leaves keys typed into a field alone', () => {
    expect(typingIn(document.createElement('input'))).toBe(true)
    expect(typingIn(document.createElement('textarea'))).toBe(true)
    expect(typingIn(document.createElement('select'))).toBe(true)
    expect(typingIn(document.createElement('button'))).toBe(false)
    expect(typingIn(null)).toBe(false)
  })

  it('moves the highlight within the page', () => {
    expect(moveHighlight(-1, 'j', 3)).toBe(0)
    expect(moveHighlight(-1, 'k', 3)).toBe(2)
    expect(moveHighlight(0, 'j', 3)).toBe(1)
    expect(moveHighlight(2, 'j', 3)).toBe(2)
    expect(moveHighlight(0, 'k', 3)).toBe(0)
    expect(moveHighlight(0, 'j', 0)).toBe(-1)
  })
})
