import { describe, expect, it } from 'vitest'
import type { ImportField, ImportSource, Suggestion } from '@nosdesk/core/services/importService'

import {
  IGNORE,
  attributeKinds,
  buildConstants,
  buildIgnored,
  buildMapping,
  counts,
  createNeeds,
  detectDateOrder,
  detectDecimal,
  freeFields,
  initialRows,
  isSettled,
  kindsInFile,
  matchesFilter,
  pill,
  everyRowCreates,
  problems,
  requiredAttributeWarnings,
  requiredOnCreate,
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

  it('blocks an uncovered required field when every row creates', () => {
    const r = rows()
    r[0].target = IGNORE
    expect(problems(r, FIELDS, 'assets', [constant('kind', 'generic')], false, 'create_only')).toEqual([
      { kind: 'required', field: 'name' },
    ])
  })

  it('lets a constant with a value cover a required field', () => {
    const blocking = (constants: ConstantRow[] = [], r = rows()) =>
      problems(r, FIELDS, 'assets', constants, false, 'create_only')
    expect(blocking()).toEqual([{ kind: 'kind' }])
    expect(blocking([constant('kind', '')])).toEqual([{ kind: 'kind' }])
    expect(blocking([constant('kind', 'generic')])).toEqual([])
    const r = rows()
    r[2].target = 'kind'
    expect(blocking([], r)).toEqual([])
  })

  it('lets First name or Last name give a person their name', () => {
    const people: ImportField[] = [
      field('email', true),
      field('name', true),
      field('first_name'),
      field('last_name'),
    ]
    const r = rows()
    r[0].target = 'email'
    r[1].target = ''
    r[2].target = ''
    r[3].target = ''
    expect(createNeeds(r, people, 'users')).toEqual(['name'])
    r[2].target = 'first_name'
    expect(createNeeds(r, people, 'users')).toEqual([])
    r[2].target = ''
    r[3].target = 'last_name'
    expect(createNeeds(r, people, 'users')).toEqual([])
    // Only for people: an asset's name isn't joined.
    expect(createNeeds(r, [...people, field('kind')], 'assets')).toEqual(['name'])
  })

  it('lets a Model column give an asset its kind', () => {
    const r = rows()
    r[2].target = 'model'
    expect(problems(r, FIELDS, 'assets', [], false, 'create_only')).toEqual([])
    expect(createNeeds(r, FIELDS, 'assets')).toEqual([])
  })

  it('asks about dates the samples leave open', () => {
    expect(problems(rows(), FIELDS, 'assets', [constant('kind', 'c')], true)).toEqual([
      { kind: 'dates' },
    ])
  })
})

describe('what creating a record needs, per mode', () => {
  // Sam's moves file: a tag and a location, no name and no kind.
  const moves = () =>
    initialRows(source(['asset_tag', 'location']), [
      { index: 0, field: 'asset_tag', reason: 'exact' },
      { index: 1, field: 'location', reason: 'exact' },
    ], null)

  it('every row creates for tickets and for assets under Only create new', () => {
    expect(everyRowCreates('tickets', 'create_update')).toBe(true)
    expect(everyRowCreates('assets', 'create_only')).toBe(true)
    expect(everyRowCreates('assets', 'create_update')).toBe(false)
    expect(everyRowCreates('assets', 'update_only')).toBe(false)
    expect(everyRowCreates('users', 'create_update')).toBe(false)
  })

  it('a key is always required, other required fields only to create', () => {
    expect(requiredOnCreate('name', 'assets')).toBe(true)
    expect(requiredOnCreate('kind', 'assets')).toBe(true)
    expect(requiredOnCreate('name', 'users')).toBe(true)
    expect(requiredOnCreate('role', 'users')).toBe(true)
    expect(requiredOnCreate('email', 'users')).toBe(false)
    expect(requiredOnCreate('title', 'tickets')).toBe(false)
  })

  it('Create and update: nothing blocks, and new assets need a name and a kind', () => {
    expect(problems(moves(), FIELDS, 'assets', [], false, 'create_update')).toEqual([])
    expect(createNeeds(moves(), FIELDS, 'assets', [], 'create_update')).toEqual(['name', 'kind'])
    expect(
      createNeeds(moves(), FIELDS, 'assets', [constant('kind', 'generic')], 'create_update'),
    ).toEqual(['name'])
  })

  it('Only update existing: no row creates, so nothing is asked for', () => {
    expect(problems(moves(), FIELDS, 'assets', [], false, 'update_only')).toEqual([])
    expect(createNeeds(moves(), FIELDS, 'assets', [], 'update_only')).toEqual([])
  })

  it('Only create new: every row creates, so a name and a kind block', () => {
    expect(problems(moves(), FIELDS, 'assets', [], false, 'create_only')).toEqual([
      { kind: 'required', field: 'name' },
      { kind: 'kind' },
    ])
    expect(createNeeds(moves(), FIELDS, 'assets', [], 'create_only')).toEqual([])
  })

  it('people: the email blocks, a name and a role only warn', () => {
    const people: ImportField[] = [field('email', true), field('name', true), field('role', true)]
    const r = initialRows(source(['Email', 'Pronouns']), [], null)
    expect(problems(r, people, 'users')).toEqual([{ kind: 'required', field: 'email' }])
    r[0].target = 'email'
    expect(problems(r, people, 'users')).toEqual([])
    expect(createNeeds(r, people, 'users')).toEqual(['name', 'role'])
    expect(createNeeds(r, people, 'users', [constant('role', 'user')])).toEqual(['name'])
  })

  it('tickets: every row creates, so a title blocks', () => {
    const tickets: ImportField[] = [field('title', true)]
    const r = initialRows(source(['Body']), [], null)
    expect(problems(r, tickets, 'tickets')).toEqual([{ kind: 'required', field: 'title' }])
    expect(createNeeds(r, tickets, 'tickets')).toEqual([])
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

describe('required attributes', () => {
  const KINDS = [
    { value: 'camera', label: 'Camera' },
    { value: 'vehicle', label: 'Vehicle' },
  ]
  const attribute = (id: string, kinds: string[], requiredFor: string[] = []): ImportField => ({
    id,
    label: id,
    group: 'attribute',
    value_type: 'text',
    required: false,
    kinds,
    required_for: requiredFor,
  })
  const ATTRS = [
    attribute('fuel', ['vehicle'], ['vehicle']),
    attribute('mount', ['camera'], ['camera']),
    attribute('sensor', ['camera'], ['camera']),
    attribute('rego', ['vehicle']),
  ]
  const mapped = (targets: Record<string, string>, samples: string[] = []): MapRow[] =>
    Object.entries(targets).map(([name, target], index) => ({
      index,
      name,
      samples: target === 'kind' ? samples : [],
      target,
      suggestion: null,
    }))

  it('reads the kinds from the Kind column samples and the kind for every row', () => {
    expect(kindsInFile(mapped({ Type: 'kind' }, ['camera', ' CAMERA', 'Vehicle', 'Boat']), [], KINDS)).toEqual([
      'camera',
      'vehicle',
    ])
    expect(kindsInFile(mapped({ Name: 'name' }), [constant('kind', 'vehicle')], KINDS)).toEqual(['vehicle'])
    expect(kindsInFile(mapped({ Name: 'name' }), [], KINDS)).toEqual([])
  })

  it('names each kind whose required attributes nothing gives', () => {
    const r = mapped({ Type: 'kind', Mount: 'mount' })
    expect(requiredAttributeWarnings(r, ATTRS, [], ['camera', 'vehicle'])).toEqual([
      { kind: 'camera', needed: ['mount', 'sensor'], missing: ['sensor'] },
      { kind: 'vehicle', needed: ['fuel'], missing: ['fuel'] },
    ])
    // A value for every row gives one.
    expect(requiredAttributeWarnings(r, ATTRS, [constant('fuel', 'Diesel')], ['vehicle'])).toEqual([])
    // A kind that isn't in the file asks for nothing.
    expect(requiredAttributeWarnings(r, ATTRS, [], [])).toEqual([])
  })

  it('groups an attribute under the kinds that define it', () => {
    expect(attributeKinds(attribute('insured', ['vehicle', 'camera']), KINDS)).toEqual(['Vehicle', 'Camera'])
    expect(attributeKinds(attribute('x', ['gone']), KINDS)).toEqual(['gone'])
  })
})
