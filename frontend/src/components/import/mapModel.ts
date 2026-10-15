/**
 * The Map step's rules, kept free of Vue so they can be tested directly:
 * the starting choice for each source column, what blocks the check, what
 * each filter shows, and the mapping sent to the server.
 */
import type {
  ColumnMapping,
  ImportField,
  ImportJobType,
  ImportSource,
  Suggestion,
  SuggestionReason,
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

/**
 * One row per source column, in source order. A job that already has a
 * mapping (back from Review, or resumed) shows that mapping, with every
 * other column on "Don't import"; a fresh job shows the suggestions.
 */
export function initialRows(
  source: ImportSource,
  suggestions: Suggestion[],
  stored: ColumnMapping | null,
): MapRow[] {
  const byIndex = new Map(suggestions.map((s) => [s.index, s]))
  const storedByIndex = new Map<number, string>()
  for (const [field, index] of Object.entries(stored ?? {})) storedByIndex.set(index, field)
  return source.headers.map((h) => {
    const suggestion = byIndex.get(h.index) ?? null
    let target: string
    if (stored) target = storedByIndex.get(h.index) ?? IGNORE
    else if (suggestion) target = suggestion.field ?? IGNORE
    else target = ''
    return { index: h.index, name: h.name, samples: h.samples, target, suggestion }
  })
}

/** The suggestion pill a row shows: only while the row still holds the
 *  suggested field. */
export function pillReason(row: MapRow): SuggestionReason | null {
  const s = row.suggestion
  if (!s || s.reason === 'report' || !s.field) return null
  return row.target === s.field ? s.reason : null
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

/** What stops the check, in the order the admin should read them. */
export function problems(
  rows: MapRow[],
  fields: ImportField[],
  jobType: ImportJobType,
  defaultKind: string | null,
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
  for (const field of fields) {
    if (!field.required || byField.has(field.id)) continue
    if (jobType === 'assets' && field.id === 'kind') {
      if (!defaultKind) out.push({ kind: 'kind' })
    } else {
      out.push({ kind: 'required', field: field.id })
    }
  }
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
  return (
    clashingColumns(row, found).length > 0 || pillReason(row) === 'synonym' || row.target === ''
  )
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
