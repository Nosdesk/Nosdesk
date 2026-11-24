// Shared reading of timeline entries and records: dates, type and outcome
// names, the values a row surfaces, and the mark a lifecycle state takes.
import { parseDateTime, toZoned } from '@internationalized/date'
import { formatDate, formatTime, getDateConfig, type DatePreset } from '@nosdesk/core/utils/dateUtils'
import type {
  AssetRecord,
  AssetRecordOutcomeCategory,
  AssetRecordType,
  AssetTimelineEntry,
} from '@nosdesk/core/types/asset'
import type { IconName } from '@/components/common/icons'
import { recordTypeName } from '@/components/admin/assetRecordTypes/recordTypeForm'
import { assetStatusTextClass, metaForAssetStatus } from '@/utils/assetStatusMeta'

export type Translate = (key: string, args?: Record<string, string | number>) => string

/** A calendar day (`YYYY-MM-DD`) as text. Formatted at noon UTC so no
 *  timezone can move it to the day before or after. */
export function formatDay(day: string, preset: DatePreset = 'date'): string {
  return formatDate(`${day}T12:00:00Z`, preset, 'UTC')
}

/** The clock time of a moment, in the viewer's timezone. */
export function formatClock(at: string): string {
  return formatTime(at)
}

/** The zone the viewer reads dates in. The record form passes the
 *  workspace's own zone instead, which the server reads a record's day in. */
function viewerZone(): string {
  return getDateConfig().defaultTimezone
}

/** The day of a moment in `zone`, as `YYYY-MM-DD`. */
export function dayOf(moment: string | Date, zone: string = viewerZone()): string {
  const parts = new Intl.DateTimeFormat('en-CA', {
    timeZone: zone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).formatToParts(typeof moment === 'string' ? new Date(moment) : moment)
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? ''
  return `${get('year')}-${get('month')}-${get('day')}`
}

export function todayDay(zone: string = viewerZone()): string {
  return dayOf(new Date(), zone)
}

/** A moment's wall-clock time (`HH:mm`) in `zone`. */
export function clockOf(at: string | null, zone: string = viewerZone()): string {
  if (!at) return ''
  const parts = new Intl.DateTimeFormat('en-GB', {
    timeZone: zone,
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }).formatToParts(new Date(at))
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? '00'
  return `${get('hour')}:${get('minute')}`
}

/** A day and a wall-clock time in `zone`, as an instant. */
export function momentOf(day: string, clock: string, zone: string = viewerZone()): string {
  return toZoned(parseDateTime(`${day}T${clock}`), zone).toAbsoluteString()
}

/** Whole days from one calendar day to another. */
export function daysBetweenDays(from: string, to: string): number {
  return Math.round((Date.parse(`${to}T12:00:00Z`) - Date.parse(`${from}T12:00:00Z`)) / 86_400_000)
}

/** Written down more than a day after it happened: the timeline says so,
 *  so backdating is visible rather than hidden. */
export function recordedLater(entry: Pick<AssetTimelineEntry, 'day' | 'recorded_at'>): boolean {
  return daysBetweenDays(entry.day, dayOf(entry.recorded_at)) > 1
}

/** How long ago a day was, in the largest unit that reads well. */
export function agoText(t: Translate, day: string, today: string = todayDay()): string {
  const days = daysBetweenDays(day, today)
  if (days <= 0) return t('asset-record-ago-today')
  if (days === 1) return t('asset-record-ago-yesterday')
  if (days < 60) return t('asset-record-ago-days', { count: days })
  if (days < 730) return t('asset-record-ago-months', { count: Math.round(days / 30.44) })
  return t('asset-record-ago-years', { count: Math.floor(days / 365.25) })
}

export function typeName(t: Translate, type: AssetRecordType | undefined, slug?: string | null): string {
  if (type) return recordTypeName(t, type)
  return slug ? t(`asset-record-type-${slug}`) : t('asset-timeline-record-fallback')
}

export function outcomeName(t: Translate, type: AssetRecordType | undefined, key: string | null): string {
  if (!key) return ''
  const outcome = type?.outcomes.find((o) => o.key === key)
  if (outcome?.label) return outcome.label
  return type ? t(`asset-record-outcome-${type.slug}-${key}`) : key
}

export function outcomeCategory(
  type: AssetRecordType | undefined,
  key: string | null,
): AssetRecordOutcomeCategory | null {
  return type?.outcomes.find((o) => o.key === key)?.category ?? null
}

export interface FieldProp {
  type?: string
  format?: string
  title?: string
  items?: { type?: string }
}

export function fieldProps(type: AssetRecordType | undefined): [string, FieldProp][] {
  if (!type) return []
  const properties = (type.field_schema.properties ?? {}) as Record<string, FieldProp>
  const ordered = type.field_order.filter((key) => key in properties)
  const rest = Object.keys(properties).filter((key) => !ordered.includes(key))
  return [...ordered, ...rest].map((key) => [key, properties[key]])
}

/** A type field's title: the workspace's own, else the built-in's
 *  catalogue name. */
export function fieldTitle(t: Translate, type: AssetRecordType, key: string, prop: FieldProp): string {
  return prop.title ?? t(`asset-record-field-${type.slug}-${key}`)
}

const TRAILING_UNIT = /^(.*?)\s*\(([^()]+)\)$/

/** "Odometer (km)" reads as the name "Odometer" and the unit "km". */
export function splitUnit(title: string): { name: string; unit: string | null } {
  const match = TRAILING_UNIT.exec(title)
  return match ? { name: match[1], unit: match[2] } : { name: title, unit: null }
}

function formatNumber(value: number): string {
  return new Intl.NumberFormat(getDateConfig().defaultLocale, { maximumFractionDigits: 2 }).format(value)
}

/** A field's value as the record shows it. */
export function fieldValueText(t: Translate, prop: FieldProp, value: unknown): string {
  if (value === null || value === undefined || value === '') return ''
  if (prop.type === 'boolean') return value ? t('asset-record-value-yes') : t('asset-record-value-no')
  if (Array.isArray(value)) return value.map(String).join(', ')
  if (typeof value === 'number') return formatNumber(value)
  if (prop.format === 'date' && typeof value === 'string') return formatDay(value)
  return String(value)
}

/** The readings a row surfaces: the type's number fields that carry a
 *  unit in their title ("182,400 km", "4,812 h"). Distance and operating
 *  hours live here until meters exist. */
export function readings(type: AssetRecordType | undefined, record: AssetRecord): string[] {
  const out: string[] = []
  for (const [key, prop] of fieldProps(type)) {
    const value = record.attributes[key]
    if (typeof value !== 'number' || (prop.type !== 'number' && prop.type !== 'integer')) continue
    const { unit } = splitUnit(prop.title ?? '')
    if (unit) out.push(`${formatNumber(value)} ${unit}`)
    if (out.length === 2) break
  }
  return out
}

export function fileSizeText(bytes: number | null | undefined): string {
  if (!bytes) return ''
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/** The mark a lifecycle state takes on the timeline: the icon and text
 *  colour the state has everywhere else, so the timeline and the status
 *  chip on the asset page say the same thing. */
export interface StatusMark {
  icon: IconName
  colorClass: string
  /** The translation key of the state's name. */
  labelKey: string
}

export function statusMark(status: string): StatusMark {
  const meta = metaForAssetStatus(status)
  return { icon: meta.icon, colorClass: assetStatusTextClass(status), labelKey: meta.labelKey }
}
