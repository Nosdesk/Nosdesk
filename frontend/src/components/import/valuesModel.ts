/**
 * Match values rules, kept out of the component so they can be tested on
 * their own: which values a card lists and in what order, the filter, the
 * bulk answers, the live counts in the strip, and the order "Next problem"
 * steps through.
 *
 * An answer is the admin's edit on this screen, else the one stored on the
 * job, else what matching suggests. Values keep the order the server's
 * matching gave them (No match, Guessed, the rest), so answering one never
 * moves it under the cursor.
 *
 * The server answers a model per the row's manufacturer, one column per
 * manufacturer; they share one Model card, each value under its
 * manufacturer. A manufacturer answer can move rows to another
 * manufacturer, so the Model card is read again for the answers given here
 * ([`previewAnswers`], [`withModels`]) before "Check again".
 */
import type {
  ImportValue,
  ImportValueColumn,
  ImportValues,
  ValueAnswer,
  ValueAnswers,
} from '@nosdesk/core/services/importService'

/** Values a card shows before "Show N more". */
export const PAGE = 25
/** Above this many values to read, a card shows counts and bulk answers only. */
export const SCALE_LIMIT = 200

/** Edits made on this screen, by [`valueId`]. */
export type Edits = Record<string, ValueAnswer>

export type ValueFilter = 'no_match' | 'guessed' | 'all'
export type BulkAction = 'skip' | 'leave_blank' | 'keep_text' | 'create'
export type Consequence =
  | 'skipped'
  | 'blank'
  | 'as_is'
  | 'text'
  | 'off'
  | 'unassigned'
  | 'created'
  | null

/** One value of a card, with the column it belongs to. */
export interface Entry {
  col: ImportValueColumn
  v: ImportValue
}

/** A value's id across cards. A field answered the same on every row is
 *  `field:key`; one answered per kind (an attribute, whose id may hold a
 *  `:`) carries its kind as well. */
export const valueId = (field: string, key: string, context = '') =>
  context ? JSON.stringify([field, context, key]) : `${field}:${key}`

/** The id of a value of `col`. */
export const idOf = (col: ImportValueColumn, v: ImportValue) =>
  valueId(col.field, v.key, col.context ?? '')

/** The id of an entry. */
export const entryId = (e: Entry) => idOf(e.col, e.v)

/** Whether a column answers models: one per manufacturer, on one card. */
export const isModel = (col: ImportValueColumn) => col.lookup === 'model'

/** A card's id: its field, and its kind when answered per kind. A model's
 *  manufacturers share one card. */
export const cardId = (col: ImportValueColumn) =>
  col.context && !isModel(col) ? JSON.stringify([col.field, col.context]) : col.field

/** The value as answers key it, as the server normalizes it. */
export const normalizeValue = (text: string) => text.trim().split(/\s+/).join(' ').toLowerCase()

export function sameAnswer(a?: ValueAnswer, b?: ValueAnswer): boolean {
  return JSON.stringify(a ?? null) === JSON.stringify(b ?? null)
}

/** The answer the last check used. */
export function checkedAnswer(v: ImportValue): ValueAnswer | undefined {
  return v.answer ?? v.suggested
}

/** The answer the rows take as things stand. */
export function currentAnswer(
  col: ImportValueColumn,
  v: ImportValue,
  edits: Edits,
): ValueAnswer | undefined {
  return edits[idOf(col, v)] ?? checkedAnswer(v)
}

export function isAnswered(col: ImportValueColumn, v: ImportValue, edits: Edits): boolean {
  return idOf(col, v) in edits || v.answer !== undefined
}

/** Whether Leave blank leaves the field as it is on an existing record as
 *  well as blank on a new one: an asset's status, which a new asset starts
 *  in service. */
export function leavesAsIs(col: ImportValueColumn): boolean {
  return col.field === 'status'
}

/** Whether Leave blank leaves the value off: a group isn't added, a
 *  person isn't assigned. */
export function leavesOff(col: ImportValueColumn): boolean {
  return col.lookup === 'group' || col.lookup === 'person'
}

/** Whether the import can create the entry a value names: a lookup that
 *  creates (not a person; a model only under a manufacturer it can belong
 *  to), for a value no entry is named. */
export function canCreate(col: ImportValueColumn, v: ImportValue): boolean {
  if (!col.lookup || col.lookup === 'person' || col.can_create === false || v.blank) return false
  return v.reason?.code !== 'ambiguous' && v.reason?.code !== 'needs_manufacturer'
}

/** An answer that drops what the file says. */
export function isDiscard(a?: ValueAnswer): boolean {
  return a?.action === 'leave_blank' || a?.action === 'skip' || a?.action === 'keep_text'
}

/** Whether a card lists the value rather than counting it as matched. A
 *  saved answer that drops data stays listed, so a discard is always read. */
export function isListed(col: ImportValueColumn, v: ImportValue, edits: Edits): boolean {
  if (isAnswered(col, v, edits)) return true
  if (v.match === 'guessed' || v.match === 'no_match') return true
  if (v.match === 'saved') return isDiscard(v.suggested)
  return false
}

/** Listed and not answered yet: a value still asking to be read. */
export function needsAnswer(col: ImportValueColumn, v: ImportValue, edits: Edits): boolean {
  return isListed(col, v, edits) && !isAnswered(col, v, edits)
}

const RANK: Record<ImportValue['match'], number> = {
  no_match: 0,
  guessed: 1,
  saved: 2,
  exact: 3,
}

/** A card's listed values: No match, then Guessed, then the rest, most
 *  rows first within each. */
export function listedValues(col: ImportValueColumn, edits: Edits): ImportValue[] {
  return col.values
    .filter((v) => isListed(col, v, edits))
    .map((v, i) => ({ v, i }))
    .sort((a, b) => RANK[a.v.match] - RANK[b.v.match] || b.v.rows.length - a.v.rows.length || a.i - b.i)
    .map(({ v }) => v)
}

/** A card's listed values across its columns, ordered as
 *  [`listedValues`] orders one column's. */
export function listedEntries(columns: ImportValueColumn[], edits: Edits): Entry[] {
  return columns
    .flatMap((col) => col.values.map((v) => ({ col, v })))
    .map((e, i) => ({ e, i }))
    .filter(({ e }) => isListed(e.col, e.v, edits))
    .sort(
      (a, b) =>
        RANK[a.e.v.match] - RANK[b.e.v.match] ||
        b.e.v.rows.length - a.e.v.rows.length ||
        a.i - b.i,
    )
    .map(({ e }) => e)
}

/** Values matched without a question that the server listed. */
export function matchedValues(col: ImportValueColumn, edits: Edits): ImportValue[] {
  return col.values.filter((v) => !isListed(col, v, edits))
}

/** A card's values matched without a question that the server listed. */
export function cardMatched(card: Card, edits: Edits): Entry[] {
  return card.columns.flatMap((col) => matchedValues(col, edits).map((v) => ({ col, v })))
}

/** Every value of a card matched without a question, counted. */
export function cardMatchedCount(card: Card, edits: Edits): number {
  return card.columns.reduce((n, col) => n + matchedCount(col, edits), 0)
}

/** Every value matched without a question, counted. */
export function matchedCount(col: ImportValueColumn, edits: Edits): number {
  const savedOrBlank = col.values.filter(
    (v) => v.match !== 'exact' && !isListed(col, v, edits),
  ).length
  return col.matched + savedOrBlank
}

export function filterValues(entries: Entry[], filter: ValueFilter): Entry[] {
  if (filter === 'all') return entries
  return entries.filter((e) => e.v.match === filter)
}

export interface Card {
  /** [`cardId`] of its columns. */
  id: string
  /** The first of its columns, for the field and the header. */
  column: ImportValueColumn
  /** One column, or a model's for each manufacturer. */
  columns: ImportValueColumn[]
  listed: Entry[]
  /** Too many values to list one by one. */
  scaled: boolean
}

/** Columns sharing a card, in source order: a model's manufacturers
 *  together. */
export function groupColumns(columns: ImportValueColumn[]): ImportValueColumn[][] {
  const groups = new Map<string, ImportValueColumn[]>()
  for (const col of [...columns].sort((a, b) => a.index - b.index)) {
    const id = cardId(col)
    groups.set(id, [...(groups.get(id) ?? []), col])
  }
  return [...groups.values()]
}

/** One card per column with a listed value, in source order. */
export function cards(view: ImportValues, edits: Edits): Card[] {
  return groupColumns(view.columns)
    .map((columns) => {
      const listed = listedEntries(columns, edits)
      return {
        id: cardId(columns[0]),
        column: columns[0],
        columns,
        listed,
        scaled: listed.length > SCALE_LIMIT,
      }
    })
    .filter((c) => c.listed.length > 0)
}

/** Columns whose every value matched, in source order. A column answered
 *  per kind is settled when it is for every kind. */
export function settledColumns(view: ImportValues, edits: Edits): ImportValueColumn[] {
  const open = new Set(
    view.columns
      .filter((col) => col.values.some((v) => isListed(col, v, edits)))
      .map((col) => col.index),
  )
  return [...view.columns]
    .sort((a, b) => a.index - b.index)
    .filter((col) => !open.has(col.index))
}

/** Whether a fresh check needs the step: some value asks to be read. */
export function needsValues(view: ImportValues): boolean {
  return view.columns.some((col) => col.values.some((v) => needsAnswer(col, v, {})))
}

/** Source columns with a value still asking to be read, on any kind. */
export function columnsNeeding(view: ImportValues, edits: Edits): number {
  return new Set(
    view.columns
      .filter((col) => col.values.some((v) => needsAnswer(col, v, edits)))
      .map((col) => col.index),
  ).size
}

/** Rows that would be skipped as things stand: the ones no answer explains,
 *  rows of every value answered Skip, and rows the last check skipped for a
 *  value whose answer hasn't changed since. */
export function skippedNow(view: ImportValues, edits: Edits): number {
  const rows = new Set(view.skipped_other)
  for (const col of view.columns) {
    for (const v of col.values) {
      const now = currentAnswer(col, v, edits)
      if (now?.action === 'skip') v.rows.forEach((r) => rows.add(r))
      else if (sameAnswer(now, checkedAnswer(v))) v.skipped_rows.forEach((r) => rows.add(r))
    }
  }
  return rows.size
}

/** What the rows holding the value come to, for the line beside the picker. */
export function consequence(col: ImportValueColumn, v: ImportValue, edits: Edits): Consequence {
  const now = currentAnswer(col, v, edits)
  if (now?.action === 'skip') return 'skipped'
  if (now?.action === 'keep_text') return 'text'
  if (now?.action === 'create') return 'created'
  if (now?.action !== 'leave_blank') return null
  if (col.lookup === 'group') return 'off'
  if (col.lookup === 'person') return 'unassigned'
  if (leavesAsIs(col)) return 'as_is'
  if (!v.blank) return 'blank'
  // A blank kind left as it is skips the new rows the last check found.
  return sameAnswer(now, checkedAnswer(v)) && v.skipped_rows.length > 0 ? 'skipped' : 'as_is'
}

/** Whether a value is still open to a bulk answer: not linked to an entry
 *  and not replaced, so it drops what the file says or creates an entry. */
function unlinked(col: ImportValueColumn, v: ImportValue, edits: Edits): boolean {
  const now = currentAnswer(col, v, edits)
  return now === undefined || isDiscard(now) || now.action === 'create'
}

/** The entries a bulk answer applies to: listed ones not linked or
 *  replaced, and for Create, only those the import can create. */
export function bulkTargets(card: Card, edits: Edits, action: BulkAction): Entry[] {
  return card.listed.filter(
    (e) => !e.v.blank && unlinked(e.col, e.v, edits) && (action !== 'create' || canCreate(e.col, e.v)),
  )
}

/** The bulk answers a card's kind of column offers. */
export function bulkKinds(col: ImportValueColumn): BulkAction[] {
  switch (col.lookup) {
    case 'manufacturer':
    case 'model':
      return ['create', 'keep_text']
    case 'group':
      return ['create', 'leave_blank']
    case 'person':
      return ['leave_blank', 'skip']
    default:
      return col.required ? ['skip'] : ['leave_blank', 'skip']
  }
}

/** The bulk answers a card offers, each with how many values it sets. One
 *  value is answered with its own picker, and an answer every target
 *  already has isn't offered. */
export function bulkActions(card: Card, edits: Edits): { action: BulkAction; count: number }[] {
  return bulkKinds(card.column)
    .map((action) => ({ action, targets: bulkTargets(card, edits, action) }))
    .filter(
      ({ action, targets }) =>
        targets.length >= 2 && targets.some((e) => currentAnswer(e.col, e.v, edits)?.action !== action),
    )
    .map(({ action, targets }) => ({ action, count: targets.length }))
}

/** Every bulk target answered with `action`. */
export function applyBulk(card: Card, edits: Edits, action: BulkAction): Edits {
  const next = { ...edits }
  for (const e of bulkTargets(card, edits, action)) next[entryId(e)] = { action }
  return next
}

/** What a card with too many values to list can answer for all of them. */
export function scaleKinds(col: ImportValueColumn): BulkAction[] {
  switch (col.lookup) {
    case 'manufacturer':
    case 'model':
      return ['keep_text', 'create', 'skip']
    case 'group':
      return ['leave_blank', 'create', 'skip']
    case 'person':
      return ['leave_blank', 'skip']
    default:
      return col.required ? ['skip'] : ['leave_blank', 'skip']
  }
}

/** The values of a scaled card one answer applies to: every listed one,
 *  and for Create, those the import can create. */
function scaleTargets(card: Card, action: BulkAction): Entry[] {
  return card.listed.filter((e) => action !== 'create' || canCreate(e.col, e.v))
}

/** The answer a scaled card's values share, if one does. */
export function scaleChoice(card: Card, edits: Edits): BulkAction | '' {
  return (
    scaleKinds(card.column).find((action) => {
      const targets = scaleTargets(card, action)
      return targets.length > 0 && targets.every((e) => currentAnswer(e.col, e.v, edits)?.action === action)
    }) ?? ''
  )
}

/** A scaled card's values answered with `action`. */
export function applyScale(card: Card, edits: Edits, action: BulkAction): Edits {
  const next = { ...edits }
  for (const e of scaleTargets(card, action)) next[entryId(e)] = { action }
  return next
}

/** The value ids in the order the cards show them. */
export function displayOrder(list: Card[]): string[] {
  return list.flatMap((card) => card.listed.map(entryId))
}

/** Values "Next problem" stops at: ones whose rows would be skipped, and
 *  ones with no match nobody has answered. */
export function problemIds(list: Card[], edits: Edits): string[] {
  return list.flatMap((card) =>
    card.listed
      .filter(
        (e) =>
          consequence(e.col, e.v, edits) === 'skipped' ||
          (e.v.match === 'no_match' && !isAnswered(e.col, e.v, edits)),
      )
      .map(entryId),
  )
}

/** The problem after `last` in display order, wrapping round. */
export function nextProblem(order: string[], problems: string[], last: string | null): string | null {
  if (problems.length === 0) return null
  const from = last === null ? -1 : order.indexOf(last)
  return problems.find((id) => order.indexOf(id) > from) ?? problems[0]
}

/** The manufacturer the rows of a model column have: the catalog's, by
 *  its name, or the file's spelling of one the import creates or keeps as
 *  text. Empty for rows without one. */
export function makerName(view: ImportValues, col: ImportValueColumn): string {
  if (!col.context) return ''
  const fromFile = view.columns
    .filter((c) => c.lookup === 'manufacturer')
    .flatMap((c) => c.values)
    .find((v) => v.key === col.context)
  const catalog = col.context_label && col.context_label !== col.context ? col.context_label : undefined
  return catalog ?? fromFile?.text ?? col.context_label ?? col.context
}

/** The manufacturer answers given on this screen that differ from the ones
 *  the server matched models under, as a stable key; empty when none do. */
export function makerEditsKey(view: ImportValues, edits: Edits): string {
  const changed: [string, ValueAnswer][] = []
  for (const col of view.columns) {
    if (col.lookup !== 'manufacturer') continue
    for (const v of col.values) {
      const edit = edits[idOf(col, v)]
      if (edit && !sameAnswer(edit, checkedAnswer(v))) changed.push([v.key, edit])
    }
  }
  return changed.length ? JSON.stringify(changed.sort((a, b) => a[0].localeCompare(b[0]))) : ''
}

/** The answers to read the Model card again under: the job's own, with
 *  this screen's manufacturer edits on top. Model edits stay on this
 *  screen, so the job's own model answers keep reading as stored. */
export function previewAnswers(view: ImportValues, edits: Edits): ValueAnswers {
  const out: ValueAnswers = {}
  for (const col of view.columns) {
    for (const v of col.values) {
      const edit = col.lookup === 'manufacturer' ? edits[idOf(col, v)] : undefined
      const answer = edit ?? v.answer
      if (!answer) continue
      const context = col.context ?? ''
      out[col.field] ??= {}
      out[col.field][context] ??= {}
      out[col.field][context][v.key] = answer
    }
  }
  return out
}

/** The view with its Model card as `preview` reads it. Model edits for a
 *  manufacturer the rows no longer have are dropped with their values. */
export function withModels(view: ImportValues, preview: ImportValues | undefined): ImportValues {
  if (!preview) return view
  const models = preview.columns.filter(isModel)
  return { ...view, columns: [...view.columns.filter((c) => !isModel(c)), ...models] }
}

/** Whether the Model card reads manufacturer answers: some manufacturer
 *  value is answered, here or on the job. */
export function makersAnswered(view: ImportValues, edits: Edits): boolean {
  return view.columns.some(
    (col) => col.lookup === 'manufacturer' && col.values.some((v) => isAnswered(col, v, edits)),
  )
}

/** The answers to store: the job's own with this screen's edits on top. */
export function answersToSend(view: ImportValues, edits: Edits): ValueAnswers {
  const out: ValueAnswers = {}
  for (const col of view.columns) {
    for (const v of col.values) {
      const answer = edits[idOf(col, v)] ?? v.answer
      if (!answer) continue
      const context = col.context ?? ''
      out[col.field] ??= {}
      out[col.field][context] ??= {}
      out[col.field][context][v.key] = answer
    }
  }
  return out
}

/** Values answered Replace with and nothing typed. */
export function emptyReplacements(view: ImportValues, edits: Edits): string[] {
  return view.columns.flatMap((col) =>
    col.values
      .filter((v) => {
        const a = currentAnswer(col, v, edits)
        return a?.action === 'replace' && a.value.trim() === ''
      })
      .map((v) => idOf(col, v)),
  )
}

/** A picker entry's value for an answer. */
export function answerKey(a?: ValueAnswer): string {
  if (!a) return ''
  return a.action === 'match' ? `match:${a.target}` : a.action
}

/** The answer a picker entry stands for; Replace with keeps the text
 *  typed before. */
export function answerFromKey(key: string, previous?: ValueAnswer): ValueAnswer {
  if (key.startsWith('match:')) return { action: 'match', target: key.slice('match:'.length) }
  if (key === 'replace') {
    return { action: 'replace', value: previous?.action === 'replace' ? previous.value : '' }
  }
  return { action: key as 'skip' | 'leave_blank' | 'keep_text' | 'create' }
}

/** A cell as a spreadsheet won't run it as a formula. */
function csvCell(text: string): string {
  const safe = /^[=+\-@\t\r]/.test(text) ? `'${text}` : text
  return /[",\n\r]/.test(safe) ? `"${safe.replace(/"/g, '""')}"` : safe
}

/** The values as CSV: the value and its row count, one per line. */
export function valuesCsv(headers: [string, string], values: ImportValue[]): string {
  const lines = [headers.map(csvCell).join(',')]
  for (const v of values) lines.push(`${csvCell(v.text)},${v.rows.length}`)
  return `${lines.join('\r\n')}\r\n`
}
