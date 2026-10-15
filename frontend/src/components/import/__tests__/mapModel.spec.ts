import { describe, expect, it } from 'vitest'
import type { ImportField, ImportSource, Suggestion } from '@nosdesk/core/services/importService'

import {
  IGNORE,
  buildMapping,
  counts,
  initialRows,
  matchesFilter,
  pillReason,
  problems,
  type MapRow,
} from '../mapModel'

const field = (id: string, required = false, value_type: ImportField['value_type'] = 'text'): ImportField => ({
  id,
  label: id,
  group: 'core',
  value_type,
  required,
})

const FIELDS: ImportField[] = [
  field('name', true),
  field('kind', true, 'enum'),
  field('asset_tag'),
  field('serial_number'),
  field('quantity', false, 'number'),
]

function source(names: string[]): ImportSource {
  return {
    format: 'csv',
    sheets: [],
    sheet: null,
    header_row: 1,
    headers: names.map((name, index) => ({ index, name, samples: [] })),
    row_count: 3,
    delimiter: ',',
  }
}

const SUGGESTIONS: Suggestion[] = [
  { index: 0, field: 'name', reason: 'exact' },
  { index: 1, field: 'serial_number', reason: 'synonym' },
  { index: 3, field: null, reason: 'report' },
]

function rows(): MapRow[] {
  return initialRows(source(['name', 'S/N', 'Extra', 'Row']), SUGGESTIONS, null)
}

describe('initialRows', () => {
  it('pre-fills suggestions, leaves the rest unchosen and report columns ignored', () => {
    expect(rows().map((r) => r.target)).toEqual(['name', 'serial_number', '', IGNORE])
  })

  it('restores a stored mapping, every other column ignored', () => {
    const restored = initialRows(source(['name', 'S/N', 'Extra', 'Row']), SUGGESTIONS, {
      name: 0,
      notes: 2,
    })
    expect(restored.map((r) => r.target)).toEqual(['name', IGNORE, 'notes', IGNORE])
    // The pill shows only while the suggestion still holds.
    expect(restored.map(pillReason)).toEqual(['exact', null, null, null])
  })
})

describe('problems', () => {
  it('blocks a field mapped twice', () => {
    const r = rows()
    r[2].target = 'name'
    expect(problems(r, FIELDS, 'assets', 'generic')).toEqual([
      { kind: 'duplicate', field: 'name', columns: [0, 2] },
    ])
  })

  it('blocks an uncovered required field', () => {
    const r = rows()
    r[0].target = IGNORE
    expect(problems(r, FIELDS, 'assets', 'generic')).toEqual([{ kind: 'required', field: 'name' }])
  })

  it('lets the default kind cover the Kind column for assets only', () => {
    expect(problems(rows(), FIELDS, 'assets', null)).toEqual([{ kind: 'kind' }])
    expect(problems(rows(), FIELDS, 'assets', 'generic')).toEqual([])
    expect(problems(rows(), FIELDS, 'users', 'generic')).toEqual([{ kind: 'required', field: 'kind' }])
    const r = rows()
    r[2].target = 'kind'
    expect(problems(r, FIELDS, 'assets', null)).toEqual([])
  })
})

describe('filters and counts', () => {
  it('needs attention: clashes, guesses and unmade choices', () => {
    const r = rows()
    const found = problems(r, FIELDS, 'assets', 'generic')
    expect(r.filter((x) => matchesFilter(x, 'attention', found)).map((x) => x.index)).toEqual([1, 2])
    r[2].target = 'name'
    const clash = problems(r, FIELDS, 'assets', 'generic')
    expect(r.filter((x) => matchesFilter(x, 'attention', clash)).map((x) => x.index)).toEqual([0, 1, 2])
  })

  it('not imported: unchosen and ignored columns', () => {
    const r = rows()
    expect(r.filter((x) => matchesFilter(x, 'ignored', [])).map((x) => x.index)).toEqual([2, 3])
    expect(counts(r, [])).toEqual({ total: 4, mapped: 2, notImported: 2, problems: 0 })
  })

  it('builds the mapping from mapped rows only', () => {
    expect(buildMapping(rows())).toEqual({ name: 0, serial_number: 1 })
  })
})
