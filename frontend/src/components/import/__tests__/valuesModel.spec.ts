import { describe, expect, it } from 'vitest'
import type {
  ImportValue,
  ImportValueColumn,
  ImportValues,
} from '@nosdesk/core/services/importService'

import {
  PAGE,
  SCALE_LIMIT,
  answerFromKey,
  answerKey,
  answersToSend,
  applyBulk,
  applyScale,
  bulkActions,
  canCreate,
  cardId,
  cardMatchedCount,
  cards,
  columnsNeeding,
  consequence,
  displayOrder,
  emptyReplacements,
  filterValues,
  listedValues,
  matchedCount,
  needsValues,
  nextProblem,
  problemIds,
  settledColumns,
  skippedNow,
  idOf,
  leavesAsIs,
  makerEditsKey,
  makerName,
  makersAnswered,
  previewAnswers,
  scaleChoice,
  valueId,
  valuesCsv,
  withModels,
  type Card,
  type Edits,
} from '../valuesModel'

function value(text: string, match: ImportValue['match'], rows: number[], extra: Partial<ImportValue> = {}): ImportValue {
  const skip = { action: 'skip' } as const
  return {
    key: text.toLowerCase(),
    text,
    blank: false,
    rows,
    match,
    suggested: match === 'no_match' ? skip : { action: 'match', target: text.toLowerCase() },
    skipped_rows: match === 'no_match' ? rows : [],
    ...extra,
  }
}

function kindColumn(values: ImportValue[], extra: Partial<ImportValueColumn> = {}): ImportValueColumn {
  return {
    field: 'kind',
    index: 1,
    header: 'Type',
    value_type: 'enum',
    required: true,
    choices: [
      { value: 'camera', label: 'Camera' },
      { value: 'camera_support', label: 'Camera support' },
    ],
    distinct: values.length,
    matched: values.filter((v) => v.match === 'exact').length,
    values,
    ...extra,
  }
}

function qtyColumn(values: ImportValue[]): ImportValueColumn {
  return {
    field: 'quantity',
    index: 6,
    header: 'Qty',
    value_type: 'number',
    required: false,
    choices: [],
    distinct: values.length + 3,
    matched: 3,
    values,
  }
}

const tripod = value('tripod', 'no_match', [18, 19, 20])
const dolly = value('dolly', 'no_match', [30])
const cam = value('Cam', 'guessed', [4, 5, 6, 7], { reason: { code: 'synonym' } })
const camra = value('Camra', 'guessed', [8])
const bcast = value('B/cast', 'saved', [9])
const spare = value('spare', 'saved', [10], { suggested: { action: 'leave_blank' } })
const camera = value('camera', 'exact', [2, 3])
const two = value('two', 'no_match', [44, 45])

function view(columns: ImportValueColumn[], skippedOther: number[] = []): ImportValues {
  return {
    rows_checked: 412,
    rows_skipped: 0,
    skipped_other: skippedOther,
    columns,
  }
}

describe('a card', () => {
  it('lists No match first, then Guessed, then the rest, most rows first', () => {
    const col = kindColumn([camera, camra, cam, bcast, spare, dolly, tripod])
    expect(listedValues(col, {}).map((v) => v.text)).toEqual(['tripod', 'dolly', 'Cam', 'Camra', 'spare'])
  })

  it('keeps an answered value where it was', () => {
    const col = kindColumn([cam, tripod, dolly])
    const edits: Edits = { [valueId('kind', 'tripod')]: { action: 'match', target: 'camera_support' } }
    expect(listedValues(col, edits).map((v) => v.text)).toEqual(['tripod', 'dolly', 'Cam'])
  })

  it('counts exact and saved non-discarding values as matched; a saved discard stays listed', () => {
    const col = kindColumn([camera, bcast, spare, tripod])
    expect(matchedCount(col, {})).toBe(2)
    expect(listedValues(col, {}).map((v) => v.text)).toContain('spare')
  })

  it('filters by how a value matched', () => {
    const col = kindColumn([])
    const list = [tripod, cam, dolly, camra].map((v) => ({ col, v }))
    expect(filterValues(list, 'no_match').map((e) => e.v.text)).toEqual(['tripod', 'dolly'])
    expect(filterValues(list, 'guessed').map((e) => e.v.text)).toEqual(['Cam', 'Camra'])
    expect(filterValues(list, 'all')).toHaveLength(4)
  })

  it('shows counts and bulk answers only above the scale limit', () => {
    const many = Array.from({ length: SCALE_LIMIT + 1 }, (_, i) => value(`odd-${i}`, 'no_match', [i + 2]))
    const [card] = cards(view([kindColumn(many)]), {})
    expect(card.scaled).toBe(true)
    const [small] = cards(view([kindColumn(many.slice(0, PAGE + 15))]), {})
    expect(small.scaled).toBe(false)
  })
})

describe('the cards', () => {
  it('come in source order, and settled columns are named once', () => {
    const v = view([qtyColumn([two]), kindColumn([tripod]), { ...kindColumn([camera]), field: 'role', index: 3, header: 'Role' }])
    expect(cards(v, {}).map((c) => c.column.header)).toEqual(['Type', 'Qty'])
    expect(settledColumns(v, {}).map((c) => c.header)).toEqual(['Role'])
  })

  it('are needed only when a value asks to be read', () => {
    expect(needsValues(view([kindColumn([camera, bcast])]))).toBe(false)
    expect(needsValues(view([kindColumn([camera, cam])]))).toBe(true)
    expect(needsValues(view([kindColumn([spare])]))).toBe(true)
  })
})

/** The one card of `col` as it stands with `edits`. */
function cardOf(columns: ImportValueColumn[], edits: Edits = {}): Card {
  const [card] = cards(view(columns), edits)
  return card
}

describe('bulk answers', () => {
  it('offer Skip all for a required field and Leave blank too for an optional one', () => {
    const kinds = kindColumn([tripod, dolly, cam])
    // Both are already Skip: nothing to offer.
    expect(bulkActions(cardOf([kinds]), {})).toEqual([])
    const leftBlank: Edits = { [valueId('kind', 'dolly')]: { action: 'leave_blank' } }
    expect(bulkActions(cardOf([kinds], leftBlank), leftBlank)).toEqual([{ action: 'skip', count: 2 }])

    const qty = qtyColumn([two, value('lots', 'no_match', [50])])
    expect(bulkActions(cardOf([qty]), {})).toEqual([{ action: 'leave_blank', count: 2 }])
  })

  it('leave linked and replaced values alone', () => {
    const qty = qtyColumn([two, value('lots', 'no_match', [50]), value('many', 'no_match', [51])])
    const edits: Edits = { [valueId('quantity', 'two')]: { action: 'replace', value: '2' } }
    const next = applyBulk(cardOf([qty], edits), edits, 'leave_blank')
    expect(next[valueId('quantity', 'two')]).toEqual({ action: 'replace', value: '2' })
    expect(next[valueId('quantity', 'lots')]).toEqual({ action: 'leave_blank' })
    expect(next[valueId('quantity', 'many')]).toEqual({ action: 'leave_blank' })
  })

  it('need two values', () => {
    const qty = qtyColumn([two])
    expect(bulkActions(cardOf([qty]), {})).toEqual([])
  })
})

const keepText = { action: 'keep_text' } as const
const leaveBlank = { action: 'leave_blank' } as const

/** A lookup value nothing matches, with the field's default. */
function unmatched(text: string, rows: number[], suggested: typeof keepText | typeof leaveBlank, extra: Partial<ImportValue> = {}) {
  return value(text, 'no_match', rows, { suggested, skipped_rows: [], ...extra })
}

function makeColumn(values: ImportValue[]): ImportValueColumn {
  return {
    ...kindColumn(values, { field: 'manufacturer', index: 3, header: 'Make', required: false }),
    lookup: 'manufacturer',
    can_create: true,
    choices: [{ value: '1', label: 'Sony' }],
  }
}

function modelColumn(context: string, values: ImportValue[], extra: Partial<ImportValueColumn> = {}): ImportValueColumn {
  return {
    ...kindColumn(values, { field: 'model', index: 4, header: 'Model', required: false }),
    lookup: 'model',
    context,
    context_label: context || undefined,
    can_create: true,
    choices: [],
    ...extra,
  }
}

describe('lookup cards', () => {
  const blackmagic = unmatched('Blackmagic', [40, 41], keepText)
  const arri = unmatched('ARRI', [42], keepText)
  const corp = value('Sony Corp', 'guessed', [12], {
    reason: { code: 'close_to', label: 'Sony' },
    suggested: { action: 'match', target: '1' },
    skipped_rows: [],
  })

  it('create or keep as text a manufacturer, leaving a guessed link alone', () => {
    const card = cardOf([makeColumn([corp, blackmagic, arri])])
    expect(bulkActions(card, {})).toEqual([{ action: 'create', count: 2 }])
    const created = applyBulk(card, {}, 'create')
    expect(created[valueId('manufacturer', 'blackmagic')]).toEqual({ action: 'create' })
    expect(created[valueId('manufacturer', 'sony corp')]).toBeUndefined()
    // Created, they can all be kept as text again.
    expect(bulkActions(cardOf([makeColumn([corp, blackmagic, arri])], created), created)).toEqual([
      { action: 'keep_text', count: 2 },
    ])
    expect(consequence(makeColumn([blackmagic]), blackmagic, created)).toBe('created')
  })

  it('put every manufacturer of a model on one card, each value under its own', () => {
    const ursa = unmatched('URSA', [40], keepText)
    const pocket = unmatched('Pocket 6K', [41], keepText)
    const venice = unmatched('Venice', [12], keepText)
    const loose = unmatched('Alexa', [50], keepText, { reason: { code: 'needs_manufacturer' } })
    const under = [
      modelColumn('sony', [venice], { context_label: 'Sony' }),
      modelColumn('blackmagic', [ursa, pocket]),
      modelColumn('', [loose], { can_create: false }),
    ]
    const list = cards(view(under), {})
    expect(list).toHaveLength(1)
    const [card] = list
    expect(card.id).toBe('model')
    // Most rows first; ties keep the server's order.
    expect(card.listed.map((e) => e.v.text)).toEqual(['Venice', 'URSA', 'Pocket 6K', 'Alexa'])
    // The same name under two manufacturers is two values.
    expect(idOf(under[0], venice)).not.toBe(idOf(under[1], { ...venice }))
    // Models without a manufacturer can't be created, only kept as text.
    expect(canCreate(under[2], loose)).toBe(false)
    expect(bulkActions(card, {})).toEqual([{ action: 'create', count: 3 }])
    const created = applyBulk(card, {}, 'create')
    expect(created[idOf(under[2], loose)]).toBeUndefined()
    expect(bulkActions(cards(view(under), created)[0], created)).toEqual([{ action: 'keep_text', count: 4 }])
  })

  it('name the manufacturer a model is under: the catalog name, else the file spelling', () => {
    const make = makeColumn([blackmagic])
    const v = view([make, modelColumn('sony', [], { context_label: 'Sony' }), modelColumn('blackmagic', [])])
    expect(makerName(v, v.columns[1])).toBe('Sony')
    expect(makerName(v, v.columns[2])).toBe('Blackmagic')
    expect(makerName(v, modelColumn('', []))).toBe('')
  })

  it('create or leave off a group', () => {
    const groups: ImportValueColumn = {
      ...kindColumn(
        [unmatched('Studio C', [5], leaveBlank), unmatched('OB van', [6, 7], leaveBlank)],
        { field: 'groups', index: 9, header: 'Groups', required: false },
      ),
      lookup: 'group',
      can_create: true,
    }
    const card = cardOf([groups])
    expect(bulkActions(card, {})).toEqual([{ action: 'create', count: 2 }])
    const created = applyBulk(card, {}, 'create')
    expect(bulkActions(cardOf([groups], created), created)).toEqual([{ action: 'leave_blank', count: 2 }])
    expect(consequence(groups, groups.values[0], {})).toBe('off')
  })

  it('leave unassigned or skip the people nothing matches, and never create one', () => {
    const emails = ['crew@tvs.example', 'hire@lens.example', 'nightshift@tvs.example', 'temp@agency.example']
    const people: ImportValueColumn = {
      ...kindColumn(
        emails.map((e, i) => unmatched(e, [100 + i], leaveBlank)),
        { field: 'primary_user', index: 10, header: 'Assigned To', required: false },
      ),
      lookup: 'person',
      can_create: false,
      choices: [{ value: 'u-1', label: 'Priya Nair', detail: 'priya@tvs.example' }],
    }
    const card = cardOf([people])
    expect(consequence(people, people.values[0], {})).toBe('unassigned')
    expect(canCreate(people, people.values[0])).toBe(false)
    // All four already left unassigned: Skip is the one to offer.
    expect(bulkActions(card, {})).toEqual([{ action: 'skip', count: 4 }])
    const skipped = applyBulk(card, {}, 'skip')
    expect(bulkActions(cardOf([people], skipped), skipped)).toEqual([{ action: 'leave_blank', count: 4 }])
  })

  it('answer a scaled card with Create only where the import can create', () => {
    const many = Array.from({ length: SCALE_LIMIT + 1 }, (_, i) => unmatched(`M-${i}`, [i + 2], keepText))
    const columns = [
      modelColumn('sony', many.slice(0, 150), { context_label: 'Sony' }),
      modelColumn('', many.slice(150), { can_create: false }),
    ]
    const card = cardOf(columns)
    expect(card.scaled).toBe(true)
    expect(scaleChoice(card, {})).toBe('keep_text')
    const created = applyScale(card, {}, 'create')
    expect(Object.keys(created)).toHaveLength(150)
    expect(scaleChoice(cardOf(columns, created), created)).toBe('create')
    expect(cardMatchedCount(card, {})).toBe(0)
  })
})

describe('the Model card under new manufacturer answers', () => {
  const corp = value('Sony Corp', 'guessed', [12, 13], {
    reason: { code: 'close_to', label: 'Sony' },
    suggested: { action: 'match', target: '1' },
    skipped_rows: [],
  })
  const fx6 = value('FX6', 'exact', [12], { suggested: { action: 'match', target: '9' }, skipped_rows: [] })
  const venice = unmatched('Venice', [13], keepText, { answer: { action: 'create' } })
  const make = makeColumn([corp])
  const models = modelColumn('sony', [fx6, venice], { context_label: 'Sony' })
  const checked = view([make, models])

  it('is read again only for manufacturer answers the check didn’t use', () => {
    expect(makerEditsKey(checked, {})).toBe('')
    // Confirming the guess changes nothing.
    expect(makerEditsKey(checked, { [idOf(make, corp)]: { action: 'match', target: '1' } })).toBe('')
    // A model edit doesn't either.
    expect(makerEditsKey(checked, { [idOf(models, venice)]: keepText })).toBe('')
    const kept = { [idOf(make, corp)]: keepText }
    expect(makerEditsKey(checked, kept)).not.toBe('')
    expect(makerEditsKey(checked, kept)).toBe(makerEditsKey(checked, { ...kept, [idOf(models, venice)]: keepText }))
    expect(makersAnswered(checked, kept)).toBe(true)
    expect(makersAnswered(checked, {})).toBe(false)
  })

  it('reads under the manufacturer edits and the job’s own answers, not the model edits', () => {
    const edits: Edits = { [idOf(make, corp)]: keepText, [idOf(models, venice)]: keepText }
    expect(previewAnswers(checked, edits)).toEqual({
      manufacturer: { '': { 'sony corp': keepText } },
      model: { sony: { venice: { action: 'create' } } },
    })
  })

  it('swaps in the recomputed models and drops answers that no longer apply', () => {
    const moved = modelColumn('sony corp', [unmatched('FX6', [12], keepText), unmatched('Venice', [13], keepText)], {
      can_create: false,
    })
    const preview = view([make, moved])
    const now = withModels(checked, preview)
    expect(now.columns.map((c) => [c.field, c.context])).toEqual([
      ['manufacturer', undefined],
      ['model', 'sony corp'],
    ])
    // The edit on Venice under Sony has no value to answer any more.
    const edits: Edits = { [idOf(models, venice)]: { action: 'create' } }
    expect(answersToSend(now, edits)).toEqual({})
    expect(withModels(checked, undefined)).toBe(checked)
  })
})

describe('the strip', () => {
  const v = view([kindColumn([tripod, cam, camera]), qtyColumn([two])], [90])

  it('counts rows that would be skipped as answers change', () => {
    // 3 tripods, 2 "two" rows, and row 90 skipped for another reason.
    expect(skippedNow(v, {})).toBe(6)
    const edits: Edits = {
      [valueId('kind', 'tripod')]: { action: 'match', target: 'camera_support' },
      [valueId('kind', 'cam')]: { action: 'skip' },
    }
    expect(skippedNow(v, edits)).toBe(1 + 2 + 4)
  })

  it('counts columns with a value still to read', () => {
    expect(columnsNeeding(v, {})).toBe(2)
    expect(
      columnsNeeding(v, {
        [valueId('quantity', 'two')]: { action: 'replace', value: '2' },
      }),
    ).toBe(1)
  })
})

describe('next problem', () => {
  it('steps through values that skip rows or have no match, wrapping round', () => {
    const v = view([kindColumn([tripod, cam, dolly]), qtyColumn([two])])
    const list = cards(v, {})
    const order = displayOrder(list)
    const problems = problemIds(list, {})
    const ids = ['kind:tripod', 'kind:dolly', 'quantity:two']
    expect(problems).toEqual(ids)
    expect(nextProblem(order, problems, null)).toBe(ids[0])
    expect(nextProblem(order, problems, ids[0])).toBe(ids[1])
    // From a value that isn't a problem, the next one after it on screen.
    expect(nextProblem(order, problems, 'kind:cam')).toBe(ids[2])
    expect(nextProblem(order, problems, ids[2])).toBe(ids[0])
    expect(nextProblem(order, [], null)).toBeNull()
  })
})

describe('answers', () => {
  it('are the stored ones with this screen’s edits on top, keyed by value', () => {
    const stored = { ...tripod, answer: { action: 'match', target: 'camera' } as const }
    const v = view([kindColumn([stored, dolly])])
    expect(answersToSend(v, { [valueId('kind', 'dolly')]: { action: 'skip' } })).toEqual({
      kind: { '': { tripod: { action: 'match', target: 'camera' }, dolly: { action: 'skip' } } },
    })
  })

  it('are keyed by kind for a field answered per kind', () => {
    const good = value('good', 'no_match', [3], { suggested: { action: 'leave_blank' }, skipped_rows: [] })
    const camera = kindColumn([value('good', 'exact', [2])], {
      field: 'attr:condition',
      context: 'camera',
      context_label: 'Camera',
      required: false,
    })
    const vehicle = kindColumn([good], {
      field: 'attr:condition',
      context: 'vehicle',
      context_label: 'Vehicle',
      required: false,
    })
    // The same field and value on two kinds are two values and two cards.
    expect(idOf(vehicle, good)).not.toBe(idOf(camera, camera.values[0]))
    expect(cardId(vehicle)).not.toBe(cardId(camera))
    const edits: Edits = { [idOf(vehicle, good)]: { action: 'match', target: 'Roadworthy' } }
    expect(answersToSend(view([camera, vehicle]), edits)).toEqual({
      'attr:condition': { vehicle: { good: { action: 'match', target: 'Roadworthy' } } },
    })
    expect(cards(view([camera, vehicle]), {}).map((c) => cardId(c.column))).toEqual([cardId(vehicle)])
    // One source column, however many kinds ask about it; settled only
    // when every kind is.
    expect(columnsNeeding(view([camera, vehicle]), {})).toBe(1)
    expect(settledColumns(view([camera, vehicle]), {})).toEqual([])
    expect(settledColumns(view([camera]), {})).toEqual([camera])
  })

  it('round-trip through picker keys, keeping text typed for Replace with', () => {
    expect(answerKey({ action: 'match', target: 'camera' })).toBe('match:camera')
    expect(answerFromKey('match:camera')).toEqual({ action: 'match', target: 'camera' })
    expect(answerFromKey('replace', { action: 'replace', value: '2' })).toEqual({ action: 'replace', value: '2' })
    expect(answerFromKey('replace', { action: 'skip' })).toEqual({ action: 'replace', value: '' })
    expect(answerFromKey('skip')).toEqual({ action: 'skip' })
  })

  it('name Replace with left empty', () => {
    const v = view([qtyColumn([two])])
    expect(emptyReplacements(v, { [valueId('quantity', 'two')]: { action: 'replace', value: ' ' } })).toEqual([
      valueId('quantity', 'two'),
    ])
  })

  it('say what the rows come to', () => {
    const kinds = kindColumn([tripod])
    expect(consequence(kinds, tripod, {})).toBe('skipped')
    const qty = qtyColumn([two])
    expect(consequence(qty, two, { [valueId('quantity', 'two')]: { action: 'leave_blank' } })).toBe('blank')
    const blank = value('', 'no_match', [3, 4], { blank: true, suggested: { action: 'leave_blank' }, skipped_rows: [4] })
    const withBlank = kindColumn([blank])
    expect(consequence(withBlank, blank, {})).toBe('skipped')
    expect(consequence(withBlank, blank, { [valueId('kind', '')]: { action: 'match', target: 'camera' } })).toBeNull()
  })

  it('say a person nothing matches is left unassigned', () => {
    const crew = value('crew@tvs.example', 'no_match', [7], { suggested: { action: 'leave_blank' }, skipped_rows: [] })
    const people = kindColumn([crew], { field: 'primary_user', required: false, lookup: 'person' })
    expect(consequence(people, crew, {})).toBe('unassigned')
    expect(answerFromKey('create')).toEqual({ action: 'create' })
  })

  it('say a catalog value is kept as text, or a group left off', () => {
    const blackmagic = value('Blackmagic', 'no_match', [2], { suggested: { action: 'keep_text' }, skipped_rows: [] })
    const makers = kindColumn([blackmagic], { field: 'manufacturer', required: false, lookup: 'manufacturer' })
    expect(consequence(makers, blackmagic, {})).toBe('text')
    const spare = value('Spare', 'no_match', [3], { suggested: { action: 'leave_blank' }, skipped_rows: [] })
    const groups = kindColumn([spare], { field: 'groups', required: false, lookup: 'group' })
    expect(consequence(groups, spare, {})).toBe('off')
  })

  it('say a status left blank is left as is', () => {
    const loaned = value('Loaned', 'no_match', [5], { suggested: { action: 'leave_blank' }, skipped_rows: [] })
    const status = kindColumn([loaned], { field: 'status', required: false })
    expect(leavesAsIs(status)).toBe(true)
    expect(consequence(status, loaned, {})).toBe('as_is')
    expect(consequence(status, loaned, { [valueId('status', 'loaned')]: { action: 'skip' } })).toBe('skipped')
  })
})

describe('the values download', () => {
  it('guards cells a spreadsheet would run', () => {
    const csv = valuesCsv(['Value in the file', 'Rows'], [value('=SUM(A1)', 'no_match', [2]), value('a,b', 'no_match', [3, 4])])
    expect(csv).toBe(`Value in the file,Rows\r\n'=SUM(A1),1\r\n"a,b",2\r\n`)
  })
})
