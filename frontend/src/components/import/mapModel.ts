/**
 * The Map step's rules, kept free of Vue so they can be tested directly:
 * the starting choice for each source column, its pill, what blocks the
 * check, what each filter shows, when the step folds to one line, the
 * date order and decimal mark the samples show, and what is sent to the
 * server.
 */
import type {
  ColumnMapping,
  DateOrder,
  DecimalMark,
  ImportConstant,
  ImportField,
  ImportJobType,
  ImportSource,
  Suggestion,
  SuggestionProvenance,
} from '@nosdesk/core/services/importService'

/** Select value for "Don't import". An empty string is "no choice yet". */
export const IGNORE = '__ignore__'

export interface MapRow {
  index: number
  name: string
  samples: string[]
  /** Field id, IGNORE, or '' while nothing is chosen. */
  target: string
  suggestion: Suggestion | null
}

export type MapFilter = 'all' | 'attention' | 'ignored'

export type Problem =
  | { kind: 'duplicate'; field: string; columns: number[] }
  | { kind: 'required'; field: string }
  | { kind: 'kind' }
  | { kind: 'dates' }

/** A value set for every row, as the footer edits it. */
export interface ConstantRow {
  /** Field id, or '' until one is picked. */
  field: string
  value: string
  /** "Also apply to existing": rows that update take it too. */
  applyAll: boolean
  /** Pre-filled by the step rather than chosen (users' Role). */
  guessed: boolean
}

/** Fields a constant can't set: the keys rows are matched on. */
const KEY_FIELDS: Record<ImportJobType, string[]> = {
  assets: ['asset_tag', 'serial_number'],
  users: ['email'],
  tickets: [],
}

/** What a suggestion pre-fills: its field, or Don't import. */
function suggestedTarget(s: Suggestion): string {
  return s.field ?? IGNORE
}

/**
 * One row per source column, in source order. A job that already has a
 * mapping (back from Review, or resumed) shows that mapping and the
 * columns it set to Don't import, with every other column unchosen; a
 * fresh job shows the suggestions.
 */
export function initialRows(
  source: ImportSource,
  suggestions: Suggestion[],
  stored: ColumnMapping | null,
  storedIgnored: number[] = [],
): MapRow[] {
  const byIndex = new Map(suggestions.map((s) => [s.index, s]))
  const storedByIndex = new Map<number, string>()
  for (const [field, index] of Object.entries(stored ?? {})) storedByIndex.set(index, field)
  const ignored = new Set(storedIgnored)
  return source.headers.map((h) => {
    const suggestion = byIndex.get(h.index) ?? null
    let target: string
    if (stored) target = storedByIndex.get(h.index) ?? (ignored.has(h.index) ? IGNORE : '')
    else if (suggestion) target = suggestedTarget(suggestion)
    else target = ''
    return { index: h.index, name: h.name, samples: h.samples, target, suggestion }
  })
}

/** The pill a row shows: Guessed for a synonym, Saved match for a choice
 *  from the last import, and only while the row still holds it. Exact id
 *  and label matches carry none. */
export type Pill = 'guessed' | 'saved'

export function pill(row: MapRow): Pill | null {
  const s = row.suggestion
  if (!s || row.target !== suggestedTarget(s)) return null
  if (s.reason === 'synonym') return 'guessed'
  if (s.reason === 'previous') return 'saved'
  return null
}

export function isMapped(row: MapRow): boolean {
  return row.target !== '' && row.target !== IGNORE
}

/** The mapping the server takes: field id to column index. */
export function buildMapping(rows: MapRow[]): ColumnMapping {
  const out: ColumnMapping = {}
  for (const row of rows) if (isMapped(row)) out[row.target] = row.index
  return out
}

/** The columns set to Don't import, which the next import remembers. */
export function buildIgnored(rows: MapRow[]): number[] {
  return rows.filter((r) => r.target === IGNORE).map((r) => r.index)
}

/** Fields a constant can set: not fed by a column, not a match key, not
 *  set by another constant (`except` keeps the row's own field). */
export function freeFields(
  rows: MapRow[],
  fields: ImportField[],
  jobType: ImportJobType,
  constants: ConstantRow[],
  except = '',
): ImportField[] {
  const mapped = new Set(rows.filter(isMapped).map((r) => r.target))
  const set = new Set(constants.map((c) => c.field).filter((f) => f && f !== except))
  return fields.filter(
    (f) => !mapped.has(f.id) && !set.has(f.id) && !KEY_FIELDS[jobType].includes(f.id),
  )
}

/** The constants the server takes: those with a field and a value that no
 *  column feeds. A constant only ever reaches existing records for assets. */
export function buildConstants(
  constants: ConstantRow[],
  rows: MapRow[],
  jobType: ImportJobType,
): ImportConstant[] {
  const mapped = new Set(rows.filter(isMapped).map((r) => r.target))
  return constants
    .filter((c) => c.field && c.value.trim() && !mapped.has(c.field))
    .map((c) => ({
      field: c.field,
      value: c.value.trim(),
      apply_to: jobType === 'assets' && c.applyAll ? 'all' : 'new',
    }))
}

/** What stops the check, in the order the admin should read them. A
 *  required field is covered by a column or by a constant with a value; an
 *  asset's kind also by a Model column, since a linked model gives it, and
 *  a person's name by First name or Last name columns. */
export function problems(
  rows: MapRow[],
  fields: ImportField[],
  jobType: ImportJobType,
  constants: ConstantRow[] = [],
  datesUnresolved = false,
): Problem[] {
  const out: Problem[] = []
  const byField = new Map<string, number[]>()
  for (const row of rows) {
    if (!isMapped(row)) continue
    byField.set(row.target, [...(byField.get(row.target) ?? []), row.index])
  }
  for (const [field, columns] of byField) {
    if (columns.length > 1) out.push({ kind: 'duplicate', field, columns })
  }
  const constant = new Set(constants.filter((c) => c.field && c.value.trim()).map((c) => c.field))
  for (const field of fields) {
    if (!field.required || byField.has(field.id) || constant.has(field.id)) continue
    if (jobType === 'assets' && field.id === 'kind' && byField.has('model')) continue
    // A person's name joins their first and last names.
    if (
      jobType === 'users' &&
      field.id === 'name' &&
      (byField.has('first_name') || byField.has('last_name'))
    ) {
      continue
    }
    out.push(
      jobType === 'assets' && field.id === 'kind'
        ? { kind: 'kind' }
        : { kind: 'required', field: field.id },
    )
  }
  if (datesUnresolved) out.push({ kind: 'dates' })
  return out
}

/** The other columns mapped to the same field as `row`, if any. */
export function clashingColumns(row: MapRow, found: Problem[]): number[] {
  for (const p of found) {
    if (p.kind === 'duplicate' && p.columns.includes(row.index)) {
      return p.columns.filter((c) => c !== row.index)
    }
  }
  return []
}

/** Rows the admin should look at: clashes, guesses, and unmade choices. */
export function needsAttention(row: MapRow, found: Problem[]): boolean {
  return clashingColumns(row, found).length > 0 || pill(row) === 'guessed' || row.target === ''
}

export function isNotImported(row: MapRow): boolean {
  return !isMapped(row)
}

export function matchesFilter(row: MapRow, filter: MapFilter, found: Problem[]): boolean {
  switch (filter) {
    case 'attention':
      return needsAttention(row, found)
    case 'ignored':
      return isNotImported(row)
    default:
      return true
  }
}

export interface MapCounts {
  total: number
  mapped: number
  notImported: number
  problems: number
}

export function counts(rows: MapRow[], found: Problem[]): MapCounts {
  const mapped = rows.filter(isMapped).length
  return {
    total: rows.length,
    mapped,
    notImported: rows.length - mapped,
    problems: found.length,
  }
}

/** Whether every column came from a saved match or an exact id or label
 *  and still holds it, nothing blocks, and no constant is in play: the
 *  step then folds to one line and one button. */
export function isSettled(rows: MapRow[], found: Problem[], constants: ConstantRow[]): boolean {
  if (rows.length === 0 || found.length > 0 || constants.length > 0) return false
  return rows.every((r) => {
    const s = r.suggestion
    return !!s && s.reason !== 'synonym' && r.target === suggestedTarget(s)
  })
}

/** The import the saved matches came from, when every saved row names
 *  the same one. */
export function savedFrom(rows: MapRow[]): SuggestionProvenance | null {
  const found = rows
    .map((r) => r.suggestion?.provenance)
    .filter((p): p is SuggestionProvenance => !!p)
  if (found.length === 0) return null
  return found.every((p) => p.job_id === found[0].job_id) ? found[0] : null
}

/** The samples of the columns mapped to fields of these value types. */
export function samplesOf(
  rows: MapRow[],
  fields: ImportField[],
  types: ImportField['value_type'][],
): string[] {
  const wanted = new Set(fields.filter((f) => types.includes(f.value_type)).map((f) => f.id))
  return rows.filter((r) => isMapped(r) && wanted.has(r.target)).flatMap((r) => r.samples)
}

/** Whether any mapped field reads values of these types, for showing the
 *  options that apply to them. */
export function mapsValueType(
  rows: MapRow[],
  fields: ImportField[],
  types: ImportField['value_type'][],
): boolean {
  const mapped = new Set(rows.filter(isMapped).map((r) => r.target))
  return fields.some((f) => mapped.has(f.id) && types.includes(f.value_type))
}

export interface DetectedDates {
  /** What the samples show; null when they show nothing either way. */
  order: DateOrder | null
  /** Every day-and-month sample could be read either way. */
  ambiguous: boolean
}

/** The date order the samples are written in: year first, or day or
 *  month first when a sample's first or second part is above 12. When
 *  every day-and-month sample could be either, or samples disagree, it is
 *  ambiguous. */
export function detectDateOrder(samples: string[]): DetectedDates {
  const seen = new Set<DateOrder>()
  let either = false
  for (const raw of samples) {
    const sample = raw.trim()
    if (/^\d{4}[-/.]\d{1,2}[-/.]\d{1,2}/.test(sample)) {
      seen.add('iso')
      continue
    }
    const m = /^(\d{1,2})[-/.](\d{1,2})[-/.]\d{2,4}\b/.exec(sample)
    if (!m) continue
    const first = Number(m[1])
    const second = Number(m[2])
    if (first > 12 && second <= 12) seen.add('dmy')
    else if (second > 12 && first <= 12) seen.add('mdy')
    else either = true
  }
  const dmy = seen.has('dmy')
  const mdy = seen.has('mdy')
  if (dmy !== mdy) return { order: dmy ? 'dmy' : 'mdy', ambiguous: false }
  if (dmy || either) return { order: null, ambiguous: true }
  return { order: seen.has('iso') ? 'iso' : null, ambiguous: false }
}

/** The decimal mark the samples use: the later of `.` and `,` when both
 *  appear, else a lone mark not followed by exactly three digits. `1,250`
 *  and `1.250` say nothing either way, and neither do samples that
 *  disagree. */
export function detectDecimal(samples: string[]): DecimalMark | null {
  let dot = false
  let comma = false
  for (const raw of samples) {
    const sample = raw.replace(/[^\d.,]/g, '')
    if (!/\d/.test(sample)) continue
    const lastDot = sample.lastIndexOf('.')
    const lastComma = sample.lastIndexOf(',')
    if (lastDot >= 0 && lastComma >= 0) {
      if (lastDot > lastComma) dot = true
      else comma = true
    } else if (lastDot >= 0 && !/\.\d{3}$/.test(sample)) {
      dot = true
    } else if (lastComma >= 0 && !/,\d{3}$/.test(sample)) {
      comma = true
    }
  }
  if (dot === comma) return null
  return dot ? '.' : ','
}

/** A kind as the pickers list it. */
export interface KindOption {
  value: string
  label: string
}

/**
 * The kinds the step can tell the file holds: the kind set for every row,
 * and the kinds the Kind column's samples name by slug or label. Only
 * samples are known before the check, so a kind further down the file
 * isn't seen; the check reports its rows on Review.
 */
export function kindsInFile(
  rows: MapRow[],
  constants: ConstantRow[],
  kinds: KindOption[],
): string[] {
  const bySpelling = new Map<string, string>()
  for (const k of kinds) {
    bySpelling.set(k.value.toLowerCase(), k.value)
    if (!bySpelling.has(k.label.toLowerCase())) bySpelling.set(k.label.toLowerCase(), k.value)
  }
  const found: string[] = []
  const add = (spelling: string) => {
    const slug = bySpelling.get(spelling.trim().toLowerCase())
    if (slug && !found.includes(slug)) found.push(slug)
  }
  const constant = constants.find((c) => c.field === 'kind' && c.value.trim())
  if (constant) add(constant.value)
  for (const row of rows) if (row.target === 'kind') row.samples.forEach(add)
  return found
}

/** A kind in the file whose new assets need attributes, and those of them
 *  no column or value for every row gives. */
export interface RequiredWarning {
  kind: string
  /** Field ids of every attribute the kind requires. */
  needed: string[]
  /** Of those, the ones nothing gives. */
  missing: string[]
}

export function requiredAttributeWarnings(
  rows: MapRow[],
  fields: ImportField[],
  constants: ConstantRow[],
  kindsPresent: string[],
): RequiredWarning[] {
  const given = new Set([
    ...rows.filter(isMapped).map((r) => r.target),
    ...constants.filter((c) => c.field && c.value.trim()).map((c) => c.field),
  ])
  const out: RequiredWarning[] = []
  for (const kind of kindsPresent) {
    const needed = fields.filter((f) => f.required_for?.includes(kind)).map((f) => f.id)
    const missing = needed.filter((id) => !given.has(id))
    if (missing.length > 0) out.push({ kind, needed, missing })
  }
  return out
}

/** The picker group an attribute sits in: the kinds that define it. */
export function attributeKinds(field: ImportField, kinds: KindOption[]): string[] {
  return (field.kinds ?? []).map((slug) => kinds.find((k) => k.value === slug)?.label ?? slug)
}
