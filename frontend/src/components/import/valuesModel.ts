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
export type BulkAction = 'skip' | 'leave_blank'
export type Consequence = 'skipped' | 'blank' | 'as_is' | null

/** A value's id across cards. A field answered the same on every row is
 *  `field:key`; one answered per kind (an attribute, whose id may hold a
 *  `:`) carries its kind as well. */
export const valueId = (field: string, key: string, context = '') =>
  context ? JSON.stringify([field, context, key]) : `${field}:${key}`

/** The id of a value of `col`. */
export const idOf = (col: ImportValueColumn, v: ImportValue) =>
  valueId(col.field, v.key, col.context ?? '')

/** A card's id: its field, and its kind when answered per kind. */
export const cardId = (col: ImportValueColumn) =>
  col.context ? JSON.stringify([col.field, col.context]) : col.field

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

/** Values matched without a question that the server listed. */
export function matchedValues(col: ImportValueColumn, edits: Edits): ImportValue[] {
  return col.values.filter((v) => !isListed(col, v, edits))
}

/** Every value matched without a question, counted. */
export function matchedCount(col: ImportValueColumn, edits: Edits): number {
  const savedOrBlank = col.values.filter(
    (v) => v.match !== 'exact' && !isListed(col, v, edits),
  ).length
  return col.matched + savedOrBlank
}

export function filterValues(values: ImportValue[], filter: ValueFilter): ImportValue[] {
  if (filter === 'all') return values
  return values.filter((v) => v.match === filter)
}

export interface Card {
  column: ImportValueColumn
  listed: ImportValue[]
  /** Too many values to list one by one. */
  scaled: boolean
}

/** One card per column with a listed value, in source order. */
export function cards(view: ImportValues, edits: Edits): Card[] {
  return [...view.columns]
    .sort((a, b) => a.index - b.index)
    .map((column) => {
      const listed = listedValues(column, edits)
      return { column, listed, scaled: listed.length > SCALE_LIMIT }
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
  if (now?.action !== 'leave_blank') return null
  if (!v.blank) return 'blank'
  // A blank kind left as it is skips the new rows the last check found.
  return sameAnswer(now, checkedAnswer(v)) && v.skipped_rows.length > 0 ? 'skipped' : 'as_is'
}

/** Values a bulk answer applies to: listed ones not linked or replaced. */
function bulkTargets(col: ImportValueColumn, listed: ImportValue[], edits: Edits): ImportValue[] {
  return listed.filter((v) => {
    if (v.blank) return false
    const now = currentAnswer(col, v, edits)
    return now === undefined || isDiscard(now)
  })
}

/** The bulk answers a card offers, each with how many values it sets. One
 *  value is answered with its own picker, and an answer every target
 *  already has isn't offered. */
export function bulkActions(
  col: ImportValueColumn,
  listed: ImportValue[],
  edits: Edits,
): { action: BulkAction; count: number }[] {
  const targets = bulkTargets(col, listed, edits)
  if (targets.length < 2) return []
  const actions: BulkAction[] = col.required ? ['skip'] : ['leave_blank', 'skip']
  return actions
    .filter((action) => targets.some((v) => currentAnswer(col, v, edits)?.action !== action))
    .map((action) => ({ action, count: targets.length }))
}

/** Every bulk target answered with `action`. */
export function applyBulk(
  col: ImportValueColumn,
  listed: ImportValue[],
  edits: Edits,
  action: BulkAction,
): Edits {
  const next = { ...edits }
  for (const v of bulkTargets(col, listed, edits)) {
    next[idOf(col, v)] = { action }
  }
  return next
}

/** The value ids in the order the cards show them. */
export function displayOrder(list: Card[]): string[] {
  return list.flatMap((card) => card.listed.map((v) => idOf(card.column, v)))
}

/** Values "Next problem" stops at: ones whose rows would be skipped, and
 *  ones with no match nobody has answered. */
export function problemIds(list: Card[], edits: Edits): string[] {
  return list.flatMap((card) =>
    card.listed
      .filter(
        (v) =>
          consequence(card.column, v, edits) === 'skipped' ||
          (v.match === 'no_match' && !isAnswered(card.column, v, edits)),
      )
      .map((v) => idOf(card.column, v)),
  )
}

/** The problem after `last` in display order, wrapping round. */
export function nextProblem(order: string[], problems: string[], last: string | null): string | null {
  if (problems.length === 0) return null
  const from = last === null ? -1 : order.indexOf(last)
  return problems.find((id) => order.indexOf(id) > from) ?? problems[0]
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
  return { action: key as 'skip' | 'leave_blank' }
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
