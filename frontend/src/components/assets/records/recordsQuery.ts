// The Records list's query: search, filters, period, grouping and sort,
// kept in the address bar so a filtered list is a link. The pure helpers
// read and write `route.query`; `useRecordsQuery` keeps one reactive copy
// in step with the route.
import { reactive, watch } from 'vue'
import { useRoute, useRouter, type LocationQuery } from 'vue-router'
import type { AssetRecordListFilter } from '@nosdesk/core/services/assetRecordService'

export type StatusValue = 'open' | 'completed'
export type PeriodPreset = 'last-30' | 'this-month' | 'this-year' | 'last-year'
export type GroupAxis = 'none' | 'vendor' | 'type' | 'asset' | 'month'
export type SortKey = 'date' | 'cost' | 'asset' | 'vendor'

export const PERIOD_PRESETS: PeriodPreset[] = ['last-30', 'this-month', 'this-year', 'last-year']
export const GROUP_AXES: GroupAxis[] = ['none', 'vendor', 'type', 'asset', 'month']
const SORT_KEYS: SortKey[] = ['date', 'cost', 'asset', 'vendor']

export interface RecordsQuery {
  q: string
  /** Type slugs. */
  type: Set<string>
  status: Set<StatusValue>
  vendor: Set<string>
  /** Asset ids, as text. */
  asset: Set<string>
  /** A named period, or `custom` with `from` and `to`, or none. */
  period: PeriodPreset | 'custom' | ''
  from: string
  to: string
  group: GroupAxis
  sort: SortKey
  dir: 'asc' | 'desc'
  /** The record open in the dialog. */
  record: number | null
}

const DAY = /^\d{4}-\d{2}-\d{2}$/

function one(value: LocationQuery[string]): string {
  const first = Array.isArray(value) ? value[0] : value
  return typeof first === 'string' ? first : ''
}

const list = (value: LocationQuery[string]) => new Set(one(value).split('|').filter(Boolean))

/** The query `raw` (a route's query) describes. */
export function readQuery(raw: LocationQuery): RecordsQuery {
  const from = DAY.test(one(raw.from)) ? one(raw.from) : ''
  const to = DAY.test(one(raw.to)) ? one(raw.to) : ''
  const preset = PERIOD_PRESETS.find((x) => x === one(raw.period))
  return {
    q: one(raw.q),
    type: list(raw.type),
    status: new Set(
      [...list(raw.status)].filter((s): s is StatusValue => s === 'open' || s === 'completed'),
    ),
    vendor: list(raw.vendor),
    asset: new Set([...list(raw.asset)].filter((id) => /^\d+$/.test(id))),
    period: preset ?? (from || to ? 'custom' : ''),
    from: preset ? '' : from,
    to: preset ? '' : to,
    group: GROUP_AXES.find((x) => x === one(raw.group)) ?? 'none',
    sort: SORT_KEYS.find((x) => x === one(raw.sort)) ?? 'date',
    dir: one(raw.dir) === 'asc' ? 'asc' : 'desc',
    record: Number(one(raw.record)) || null,
  }
}

const joined = (set: Set<string>) => [...set].join('|')

/** `query` as a route's query, leaving out what is at its default. */
export function writeQuery(query: RecordsQuery): Record<string, string> {
  const out: Record<string, string> = {}
  const put = (key: string, value: string | null | undefined) => {
    if (value) out[key] = value
  }
  put('q', query.q)
  put('type', joined(query.type))
  put('status', joined(query.status))
  put('vendor', joined(query.vendor))
  put('asset', joined(query.asset))
  if (query.period === 'custom') {
    put('from', query.from)
    put('to', query.to)
  } else {
    put('period', query.period)
  }
  if (query.group !== 'none') put('group', query.group)
  if (query.sort !== 'date') put('sort', query.sort)
  if (query.dir !== 'desc') put('dir', query.dir)
  if (query.record) put('record', String(query.record))
  return out
}

/** The days `query`'s period covers, inclusive; an open end is an empty
 *  string. `today` is YYYY-MM-DD. */
export function periodRange(query: RecordsQuery, today: string): { from: string; to: string } {
  const year = Number(today.slice(0, 4))
  switch (query.period) {
    case 'last-30': {
      const d = new Date(`${today}T12:00:00Z`)
      d.setUTCDate(d.getUTCDate() - 29)
      return { from: d.toISOString().slice(0, 10), to: today }
    }
    case 'this-month':
      return { from: `${today.slice(0, 7)}-01`, to: today }
    case 'this-year':
      return { from: `${year}-01-01`, to: today }
    case 'last-year':
      return { from: `${year - 1}-01-01`, to: `${year - 1}-12-31` }
    case 'custom':
      return { from: query.from, to: query.to }
    default:
      return { from: '', to: '' }
  }
}

/** The filters `query` asks the server for (without paging). */
export function listFilter(query: RecordsQuery, today: string): AssetRecordListFilter {
  const range = periodRange(query, today)
  return {
    q: query.q.trim() || undefined,
    type: [...query.type].sort(),
    status: [...query.status].sort(),
    vendor: [...query.vendor].sort(),
    asset: [...query.asset].map(Number).sort((a, b) => a - b),
    from: range.from || undefined,
    to: range.to || undefined,
    sort: query.sort,
    dir: query.dir,
    group: query.group === 'none' ? undefined : query.group,
  }
}

/** Clear the search, filters and period; grouping and sort stay. */
export function clearFilters(query: RecordsQuery): void {
  query.q = ''
  query.type.clear()
  query.status.clear()
  query.vendor.clear()
  query.asset.clear()
  query.period = ''
  query.from = ''
  query.to = ''
}

/** The list's query, kept in step with the route both ways: a change here
 *  replaces the route's query, and a new route query (a link, the tab, back)
 *  is read in. */
export function useRecordsQuery(): RecordsQuery {
  const route = useRoute()
  const router = useRouter()
  const query = reactive<RecordsQuery>(readQuery(route.query))
  let written = JSON.stringify(writeQuery(query))

  watch(
    query,
    () => {
      const next = writeQuery(query)
      const text = JSON.stringify(next)
      if (text === written) return
      written = text
      void router.replace({ query: next })
    },
    { deep: true },
  )
  watch(
    () => route.query,
    (raw) => {
      const incoming = readQuery(raw)
      const text = JSON.stringify(writeQuery(incoming))
      if (text === written) return
      written = text
      Object.assign(query, incoming)
    },
  )
  return query
}
