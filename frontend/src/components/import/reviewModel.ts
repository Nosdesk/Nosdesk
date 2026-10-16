/**
 * Review rules, kept out of the component so they can be tested on their
 * own: the four outcome counts and the filter Review opens on, where each
 * Top problems entry is fixed, the Not imported lines, and what the
 * changed-plan banner and the webhook sentence say.
 *
 * Imported means new plus updated: an unchanged row matches a record and
 * writes nothing to it, so it is counted on its own.
 */
import type {
  DroppedGroup,
  ImportField,
  ImportFix,
  ImportJob,
  ImportSummary,
  RowFilter,
} from '@nosdesk/core/services/importService'

/** Rows a page of the row list shows. */
export const PAGE = 25

export interface Outcomes {
  total: number
  created: number
  updated: number
  unchanged: number
  skipped: number
  /** New plus updated: the rows Import writes. */
  imported: number
}

export function outcomes(summary: ImportSummary): Outcomes {
  const created = summary.would_create
  const updated = summary.would_update
  const unchanged = summary.unchanged ?? 0
  const skipped =
    summary.rows_with_errors ?? Math.max(summary.row_count - created - updated - unchanged, 0)
  return {
    total: summary.row_count,
    created,
    updated,
    unchanged,
    skipped,
    imported: created + updated,
  }
}

/**
 * Whether the error report has rows: skipped ones, or ones that import
 * without some of their values (any dropped group). Review and Done show
 * its download only then.
 */
export function reportHasRows(skipped: number, summary: ImportSummary | null | undefined): boolean {
  return skipped > 0 || (summary?.dropped?.length ?? 0) > 0
}

/** Skipped rows first when there are any, else updates, else every row. */
export function defaultFilter(o: Outcomes): RowFilter {
  if (o.skipped > 0) return 'skipped'
  if (o.updated > 0) return 'update'
  return 'all'
}

export interface Problem {
  /** Index in the summary's error groups: what `?group=` downloads. */
  index: number
  column: string | null
  source_column?: string | null
  message: string
  count: number
  fix: ImportFix
}

/** The summary's error groups, each with where it is fixed. A summary
 *  checked before fixes were classified downloads its rows. */
export function problems(summary: ImportSummary): Problem[] {
  return (summary.error_groups ?? []).map((g, index) => ({
    index,
    column: g.column,
    source_column: g.source_column,
    message: g.message,
    count: g.count,
    fix: g.fix ?? 'download',
  }))
}

/** One Not imported line, in source column order. */
export type NotImportedLine =
  /** Columns no field was chosen for, as one line. */
  | { kind: 'unmapped'; key: string; columns: number[]; headers: string[] }
  /** A column set to Don't import, with how many rows hold a value. */
  | { kind: 'ignored'; key: string; columns: number[]; header: string; values: number }
  /** An attribute the rows' kind doesn't have. */
  | {
      kind: 'attribute'
      key: string
      columns: number[]
      header: string
      kindSlug: string
      rows: number
      dropped: number[]
    }
  /** A group the rows list that nothing links. */
  | {
      kind: 'group'
      key: string
      columns: number[]
      header: string
      value: string
      rows: number
      dropped: number[]
    }
  /** People who aren't members, left unassigned. */
  | {
      kind: 'person'
      key: string
      columns: number[]
      header: string
      values: number
      rows: number
      dropped: number[]
    }

export interface NotImported {
  lines: NotImportedLine[]
  /** Distinct columns of the file that lose anything. */
  columns: number
}

/** A field id as the admin sees it: the file's header, else the field's
 *  label. */
function headerOf(
  field: string,
  source: string | null | undefined,
  fields: Map<string, ImportField>,
  label: (field: ImportField) => string,
): string {
  if (source) return source
  const f = fields.get(field)
  return f ? label(f) : field
}

/** Everything in the file that won't land: columns no field takes, and
 *  values rows import without. One line for the columns with no field
 *  chosen, so a dozen of them don't bury the real drops. `label` names a
 *  field that no column maps to. */
export function notImported(
  job: ImportJob,
  fields: ImportField[],
  label: (field: ImportField) => string,
): NotImported {
  const summary = job.summary
  const headers = job.source?.headers ?? []
  const mapping = job.mapping ?? {}
  const ignored = new Set(job.ignored ?? [])
  const byId = new Map(fields.map((f) => [f.id, f]))
  const lines: (NotImportedLine & { order: number })[] = []

  const unmapped = summary?.unmapped
  if (unmapped) {
    const none = unmapped.filter((c) => !ignored.has(c.index))
    if (none.length > 0) {
      lines.push({
        kind: 'unmapped',
        key: 'unmapped',
        columns: none.map((c) => c.index),
        headers: none.map((c) => headers[c.index]?.name ?? ''),
        order: none[0].index,
      })
    }
    for (const c of unmapped.filter((c) => ignored.has(c.index))) {
      lines.push({
        kind: 'ignored',
        key: `ignored:${c.index}`,
        columns: [c.index],
        header: headers[c.index]?.name ?? '',
        values: c.values,
        order: c.index,
      })
    }
  }

  const dropped: (DroppedGroup & { index: number })[] = (summary?.dropped ?? []).map((d, index) => ({
    ...d,
    index,
  }))
  const columnOf = (field: string) => mapping[field]
  const orderOf = (field: string) => columnOf(field) ?? Number.MAX_SAFE_INTEGER
  const columnsOf = (field: string) => (columnOf(field) === undefined ? [] : [columnOf(field)!])

  // People: one line per column, every value and row together.
  const people = dropped.filter((d) => d.column === 'primary_user')
  if (people.length > 0) {
    const first = people[0]
    lines.push({
      kind: 'person',
      key: 'person:primary_user',
      columns: columnsOf(first.column),
      header: headerOf(first.column, first.source_column, byId, label),
      values: new Set(people.map((d) => (d.value ?? '').toLowerCase())).size,
      rows: people.reduce((n, d) => n + d.count, 0),
      dropped: people.map((d) => d.index),
      order: orderOf(first.column),
    })
  }

  // Groups: one line per value, whatever the rows' kinds.
  const groupLines = new Map<string, NotImportedLine & { kind: 'group'; order: number }>()
  for (const d of dropped.filter((d) => d.column !== 'primary_user' && d.value !== undefined)) {
    const key = `group:${d.column}:${d.value}`
    const line = groupLines.get(key)
    if (line) {
      line.rows += d.count
      line.dropped.push(d.index)
      continue
    }
    groupLines.set(key, {
      kind: 'group',
      key,
      columns: columnsOf(d.column),
      header: headerOf(d.column, d.source_column, byId, label),
      value: d.value!,
      rows: d.count,
      dropped: [d.index],
      order: orderOf(d.column),
    })
  }
  lines.push(...groupLines.values())

  // Attributes: one line per column and kind.
  for (const d of dropped.filter((d) => d.value === undefined)) {
    lines.push({
      kind: 'attribute',
      key: `attribute:${d.column}:${d.kind}`,
      columns: columnsOf(d.column),
      header: headerOf(d.column, d.source_column, byId, label),
      kindSlug: d.kind,
      rows: d.count,
      dropped: [d.index],
      order: orderOf(d.column),
    })
  }

  // Stable: lines of one column keep the order above.
  lines.sort((a, b) => a.order - b.order)
  const columns = new Set(lines.flatMap((l) => l.columns))
  return {
    lines: lines.map(({ order: _order, ...line }) => line as NotImportedLine),
    columns: columns.size,
  }
}

/** How the plan moved between the check the admin read and the one the
 *  commit made: `more` rows skipped (negative for fewer). */
export function planDelta(before: ImportSummary | null, after: ImportSummary | null): number {
  if (!before || !after) return 0
  return outcomes(after).skipped - outcomes(before).skipped
}

/** Whether an enabled webhook hears about assets. */
export function webhookHearsAssets(webhooks: { enabled: boolean; events: string[] }[]): boolean {
  return webhooks.some((w) => w.enabled && w.events.some((e) => e.startsWith('asset.')))
}

/** Whether a key press belongs to a field being typed in, so a single-key
 *  shortcut must leave it alone. */
export function typingIn(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null
  if (!el || typeof el.tagName !== 'string') return false
  const tag = el.tagName.toUpperCase()
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable === true
}

/** The row highlight after `j` (down) or `k` (up) on a page of `count`
 *  rows; -1 is none. */
export function moveHighlight(current: number, key: 'j' | 'k', count: number): number {
  if (count === 0) return -1
  if (current < 0) return key === 'j' ? 0 : count - 1
  const next = key === 'j' ? current + 1 : current - 1
  return Math.min(Math.max(next, 0), count - 1)
}
