/**
 * Formatting on `@nosdesk/core/utils/dateUtils`: presets follow the
 * locale's field order, ranges collapse shared parts, and relative
 * times come out of `Intl.RelativeTimeFormat` in the active locale.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  formatDate,
  formatDateRange,
  formatRelativeTime,
  parseDate,
  setDateConfig,
} from '@nosdesk/core/utils/dateUtils'

// ICU builds differ in whitespace (thin spaces in ranges) and in the
// date-time joiner (", " or " at "); the assertions allow both.
const squash = (s: string) => s.replace(/\s+/g, ' ')

beforeEach(() => setDateConfig({ defaultLocale: 'en-US', defaultTimezone: 'UTC' }))
afterEach(() => vi.useRealTimers())

describe('formatDate', () => {
  const iso = '2026-09-17T15:42:00Z'

  it('defaults to the short date', () => {
    expect(formatDate(iso)).toBe('Sep 17, 2026')
  })

  it('lets the locale order the fields', () => {
    expect(formatDate(iso, 'monthDay')).toBe('Sep 17')
    setDateConfig({ defaultLocale: 'en-AU' })
    expect(formatDate(iso, 'monthDay')).toBe('17 Sept')
    setDateConfig({ defaultLocale: 'fr' })
    expect(formatDate(iso, 'monthDay')).toBe('17 sept.')
  })

  it('renders every preset', () => {
    expect(squash(formatDate(iso, 'dateTime'))).toMatch(/^Sep 17, 2026(,| at) 3:42 PM$/)
    expect(squash(formatDate(iso, 'longDateTime'))).toMatch(/^September 17, 2026(,| at) 3:42 PM$/)
    expect(formatDate(iso, 'monthYear')).toBe('Sep 2026')
    expect(formatDate(iso, 'month')).toBe('Sep')
    expect(formatDate(iso, 'day')).toBe('17')
    expect(formatDate(iso, 'weekday')).toBe('Thu')
  })

  it('formats in the configured zone unless overridden', () => {
    setDateConfig({ defaultTimezone: 'Australia/Sydney' })
    expect(formatDate(iso, 'day')).toBe('18')
    expect(formatDate(iso, 'day', 'UTC')).toBe('17')
  })

  it('treats a zoneless timestamp as UTC', () => {
    expect(formatDate('2026-09-17T23:30:00', 'day')).toBe('17')
    setDateConfig({ defaultTimezone: 'Australia/Sydney' })
    expect(formatDate('2026-09-17T23:30:00', 'day')).toBe('18')
  })

  it('treats a bare date as UTC midnight', () => {
    expect(parseDate('2026-09-17')?.toISOString()).toBe('2026-09-17T00:00:00.000Z')
    expect(formatDate('2026-09-17')).toBe('Sep 17, 2026')
  })

  it('keeps a zone marker', () => {
    expect(parseDate('2026-09-17T09:00:00+10:00')?.toISOString()).toBe('2026-09-16T23:00:00.000Z')
    expect(parseDate('2026-09-17T15:42:00.123456Z')?.toISOString()).toBe('2026-09-17T15:42:00.123Z')
  })

  it('is empty for nothing or garbage', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    expect(formatDate(null)).toBe('')
    expect(formatDate('nope')).toBe('')
  })
})

describe('formatDateRange', () => {
  it('collapses a shared month the way the locale does', () => {
    const from = '2026-08-07T00:00:00Z'
    const to = '2026-08-13T00:00:00Z'
    expect(squash(formatDateRange(from, to))).toBe('Aug 7 – 13')
    setDateConfig({ defaultLocale: 'en-AU' })
    expect(squash(formatDateRange(from, to))).toBe('7–13 Aug')
  })

  it('spells out both months when they differ', () => {
    expect(squash(formatDateRange('2026-08-28T00:00:00Z', '2026-09-03T00:00:00Z'))).toBe(
      'Aug 28 – Sep 3',
    )
  })
})

describe('formatRelativeTime', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-09-17T12:00:00Z'))
  })

  it('speaks the active locale, past and future', () => {
    expect(formatRelativeTime('2026-09-17T09:00:00Z')).toBe('3 hours ago')
    expect(formatRelativeTime('2026-09-16T12:00:00Z')).toBe('yesterday')
    expect(formatRelativeTime('2026-09-17T15:00:00Z')).toBe('in 3 hours')
    setDateConfig({ defaultLocale: 'fr' })
    expect(formatRelativeTime('2026-09-17T09:00:00Z')).toBe('il y a 3 heures')
  })
})
