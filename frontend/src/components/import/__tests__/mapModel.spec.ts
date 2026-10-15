import { describe, expect, it } from 'vitest'
import type { ImportField, ImportSource, Suggestion } from '@nosdesk/core/services/importService'

import {
  IGNORE,
  buildConstants,
  buildIgnored,
  buildMapping,
  counts,
  detectDateOrder,
  detectDecimal,
  freeFields,
  initialRows,
  isSettled,
  matchesFilter,
  pill,
  problems,
  savedFrom,
  type ConstantRow,
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
  field('location'),
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

const constant = (field: string, value: string, extra: Partial<ConstantRow> = {}): ConstantRow => ({
  field,
  value,
  applyAll: false,
  guessed: false,
  ...extra,
})

const SAM = { job_id: 'job-sam', by: 'user-sam', by_name: 'Sam', at: '2026-07-12T00:00:00Z' }

describe('initialRows', () => {
  it('pre-fills suggestions, leaves the rest unchosen and report columns ignored', () => {
    expect(rows().map((r) => r.target)).toEqual(['name', 'serial_number', '', IGNORE])
  })

  it('restores a stored mapping and its ignored columns, the rest unchosen', () => {
    const restored = initialRows(
      source(['name', 'S/N', 'Extra', 'Row']),
      SUGGESTIONS,
      { name: 0, notes: 2 },
      [3],
    )
    expect(restored.map((r) => r.target)).toEqual(['name', '', 'notes', IGNORE])
    // A suggestion shows its pill only while the row still holds it.
    expect(restored.map(pill)).toEqual([null, null, null, null])
  })
})

describe('pills', () => {
  it('marks a guess and a saved match, and nothing for exact names', () => {
    const r = initialRows(
      source(['name', 'S/N', 'Notes', 'Row', 'Asset tag']),
      [
        { index: 0, field: 'name', reason: 'exact' },
        { index: 1, field: 'serial_number', reason: 'synonym' },
        { index: 2, field: null, reason: 'previous', provenance: SAM },
        { index: 3, field: null, reason: 'report' },
        { index: 4, field: 'asset_tag', reason: 'label' },
      ],
      null,
    )
    expect(r.map(pill)).toEqual([null, 'guessed', 'saved', null, null])
    // Only the guess needs attention.
    expect(r.filter((x) => matchesFilter(x, 'attention', [])).map((x) => x.index)).toEqual([1])
    // A saved Don't import is a choice like any other: changing it drops the pill.
    r[2].target = 'location'
    expect(pill(r[2])).toBeNull()
  })

  it('names the import every saved match came from, when they agree', () => {
    const saved = (index: number, job: string): Suggestion => ({
      index,
      field: null,
      reason: 'previous',
      provenance: { ...SAM, job_id: job },
    })
    const one = initialRows(source(['a', 'b']), [saved(0, 'j1'), saved(1, 'j1')], null)
    expect(savedFrom(one)?.job_id).toBe('j1')
    const two = initialRows(source(['a', 'b']), [saved(0, 'j1'), saved(1, 'j2')], null)
    expect(savedFrom(two)).toBeNull()
    expect(savedFrom(rows())).toBeNull()
  })
})

describe('problems', () => {
  it('blocks a field mapped twice', () => {
    const r = rows()
    r[2].target = 'name'
    expect(problems(r, FIELDS, 'assets', [constant('kind', 'generic')])).toEqual([
      { kind: 'duplicate', field: 'name', columns: [0, 2] },
    ])
  })

  it('blocks an uncovered required field', () => {
    const r = rows()
    r[0].target = IGNORE
    expect(problems(r, FIELDS, 'assets', [constant('kind', 'generic')])).toEqual([
      { kind: 'required', field: 'name' },
    ])
  })

  it('lets a constant with a value cover a required field', () => {
    expect(problems(rows(), FIELDS, 'assets')).toEqual([{ kind: 'kind' }])
    expect(problems(rows(), FIELDS, 'assets', [constant('kind', '')])).toEqual([{ kind: 'kind' }])
    expect(problems(rows(), FIELDS, 'assets', [constant('kind', 'generic')])).toEqual([])
    expect(problems(rows(), FIELDS, 'users')).toEqual([{ kind: 'required', field: 'kind' }])
    const r = rows()
    r[2].target = 'kind'
    expect(problems(r, FIELDS, 'assets')).toEqual([])
  })

  it('asks about dates the samples leave open', () => {
    expect(problems(rows(), FIELDS, 'assets', [constant('kind', 'c')], true)).toEqual([
      { kind: 'dates' },
    ])
  })
})

describe('filters and counts', () => {
  it('needs attention: clashes, guesses and unmade choices', () => {
    const r = rows()
    const found = problems(r, FIELDS, 'assets', [constant('kind', 'generic')])
    expect(r.filter((x) => matchesFilter(x, 'attention', found)).map((x) => x.index)).toEqual([1, 2])
    r[2].target = 'name'
    const clash = problems(r, FIELDS, 'assets', [constant('kind', 'generic')])
    expect(r.filter((x) => matchesFilter(x, 'attention', clash)).map((x) => x.index)).toEqual([0, 1, 2])
  })

  it('not imported: unchosen and ignored columns', () => {
    const r = rows()
    expect(r.filter((x) => matchesFilter(x, 'ignored', [])).map((x) => x.index)).toEqual([2, 3])
    expect(counts(r, [])).toEqual({ total: 4, mapped: 2, notImported: 2, problems: 0 })
  })

  it('builds the mapping from mapped rows and remembers ignored ones apart', () => {
    expect(buildMapping(rows())).toEqual({ name: 0, serial_number: 1 })
    // The unchosen Extra is not a Don't import.
    expect(buildIgnored(rows())).toEqual([3])
  })
})

describe('folding', () => {
  const settledSuggestions: Suggestion[] = [
    { index: 0, field: 'name', reason: 'exact' },
    { index: 1, field: 'kind', reason: 'previous', provenance: SAM },
    { index: 2, field: null, reason: 'previous', provenance: SAM },
    { index: 3, field: null, reason: 'report' },
  ]
  const settledRows = () =>
    initialRows(source(['name', 'Kind', 'Notes', 'Row']), settledSuggestions, null)

  it('folds when every column is saved or exact and nothing blocks', () => {
    const r = settledRows()
    expect(isSettled(r, problems(r, FIELDS, 'assets'), [])).toBe(true)
  })

  it('stays open for a guess, an unchosen column, a changed pick, a problem or a constant', () => {
    expect(isSettled(rows(), [], [])).toBe(false)

    const unchosen = initialRows(
      source(['name', 'Kind', 'Notes', 'Row', 'Term 3 notes']),
      settledSuggestions,
      null,
    )
    expect(isSettled(unchosen, [], [])).toBe(false)

    const changed = settledRows()
    changed[2].target = 'location'
    expect(isSettled(changed, [], [])).toBe(false)

    expect(isSettled(settledRows(), [{ kind: 'dates' }], [])).toBe(false)
    expect(isSettled(settledRows(), [], [constant('role', 'user', { guessed: true })])).toBe(false)
  })
})

describe('constants', () => {
  it('offers fields no column, key or other constant sets', () => {
    const ids = (list: ImportField[]) => list.map((f) => f.id)
    const r = rows()
    expect(ids(freeFields(r, FIELDS, 'assets', []))).toEqual(['kind', 'quantity', 'location'])
    const set = [constant('kind', 'generic'), constant('location', '')]
    expect(ids(freeFields(r, FIELDS, 'assets', set))).toEqual(['quantity'])
    expect(ids(freeFields(r, FIELDS, 'assets', set, 'location'))).toEqual(['quantity', 'location'])
  })

  it('sends those with a field and a value, reaching existing records for assets only', () => {
    const r = rows()
    const list = [
      constant('kind', ' camera ', { applyAll: true }),
      constant('location', ''),
      constant('', 'x'),
      constant('name', 'mapped'),
    ]
    expect(buildConstants(list, r, 'assets')).toEqual([
      { field: 'kind', value: 'camera', apply_to: 'all' },
    ])
    expect(buildConstants(list, r, 'users')).toEqual([
      { field: 'kind', value: 'camera', apply_to: 'new' },
    ])
  })
})

describe('detected options', () => {
  it('reads the date order from the samples', () => {
    expect(detectDateOrder(['14/03/2024', '01/02/2024'])).toEqual({ order: 'dmy', ambiguous: false })
    expect(detectDateOrder(['03/14/2024'])).toEqual({ order: 'mdy', ambiguous: false })
    expect(detectDateOrder(['2024-03-14'])).toEqual({ order: 'iso', ambiguous: false })
    expect(detectDateOrder(['01/02/2024', '05/06/2024'])).toEqual({ order: null, ambiguous: true })
    expect(detectDateOrder(['14/03/2024', '03/14/2024'])).toEqual({ order: null, ambiguous: true })
    expect(detectDateOrder(['soon', ''])).toEqual({ order: null, ambiguous: false })
  })

  it('reads the decimal mark from the samples', () => {
    expect(detectDecimal(['$7,450.00'])).toBe('.')
    expect(detectDecimal(['1.234,5'])).toBe(',')
    expect(detectDecimal(['12,5'])).toBe(',')
    expect(detectDecimal(['12.5'])).toBe('.')
    expect(detectDecimal(['1,250', '3'])).toBeNull()
    expect(detectDecimal(['12.5', '12,5'])).toBeNull()
  })
})
