/**
 * Calendar arithmetic on `@nosdesk/core/utils/dateMath`. Lives here
 * because core has no test runner; the vitest alias resolves the module.
 * Every case names its zone so the result is the same on any machine.
 */
import { describe, expect, it } from 'vitest'
import {
  addDays,
  addMonths,
  daysBetween,
  endOfDay,
  fromCalendarDateString,
  getLocalTimeZone,
  startOfDay,
  startOfMonth,
  startOfQuarter,
  startOfWeek,
  toCalendarDateString,
} from '@nosdesk/core/utils/dateMath'

const SYDNEY = 'Australia/Sydney'
const HOUR = 3_600_000

describe('startOfDay', () => {
  it('is midnight in the given zone, not the browser zone', () => {
    // 2026-09-17 10:00 UTC is 20:00 in Sydney; Sydney midnight is 14:00Z the day before.
    const d = startOfDay(new Date('2026-09-17T10:00:00Z'), SYDNEY)
    expect(d.toISOString()).toBe('2026-09-16T14:00:00.000Z')
  })

  it('resolves forward through a DST gap where midnight does not exist', () => {
    // Chile springs forward at 00:00 on 2026-09-06, so the day starts at 01:00.
    const d = startOfDay(new Date('2026-09-06T12:00:00Z'), 'America/Santiago')
    expect(d.toISOString()).toBe('2026-09-06T04:00:00.000Z')
    expect(toCalendarDateString(d, 'America/Santiago')).toBe('2026-09-06')
  })
})

describe('endOfDay', () => {
  it('is the last millisecond of the day in the zone', () => {
    const d = endOfDay(new Date('2026-09-17T00:00:00Z'), SYDNEY)
    expect(d.toISOString()).toBe('2026-09-17T13:59:59.999Z')
  })
})

describe('addDays', () => {
  it('keeps the wall-clock time across a spring-forward day', () => {
    // Sydney moves 02:00 -> 03:00 on 2026-10-04; a day from 09:00 is 23 hours.
    const from = new Date('2026-10-02T23:00:00Z') // 09:00 AEST on the 3rd
    const to = addDays(from, 1, SYDNEY)
    expect(to.toISOString()).toBe('2026-10-03T22:00:00.000Z') // 09:00 AEDT on the 4th
    expect(to.getTime() - from.getTime()).toBe(23 * HOUR)
  })

  it('keeps the wall-clock time across a fall-back day', () => {
    // Sydney moves 03:00 -> 02:00 on 2026-04-05; a day from 09:00 is 25 hours.
    const from = new Date('2026-04-03T22:00:00Z') // 09:00 AEDT on the 4th
    const to = addDays(from, 1, SYDNEY)
    expect(to.toISOString()).toBe('2026-04-04T23:00:00.000Z') // 09:00 AEST on the 5th
    expect(to.getTime() - from.getTime()).toBe(25 * HOUR)
  })

  it('subtracts with a negative count', () => {
    const d = addDays(new Date('2026-03-01T12:00:00Z'), -1, 'UTC')
    expect(d.toISOString()).toBe('2026-02-28T12:00:00.000Z')
  })
})

describe('addMonths', () => {
  it('constrains the day of month', () => {
    const d = addMonths(new Date('2026-01-31T00:00:00Z'), 1, 'UTC')
    expect(d.toISOString()).toBe('2026-02-28T00:00:00.000Z')
  })
})

describe('startOfWeek', () => {
  it('is the preceding Monday, across a year boundary', () => {
    // 2027-01-01 is a Friday.
    const d = startOfWeek(new Date('2027-01-01T12:00:00Z'), 'UTC')
    expect(d.toISOString()).toBe('2026-12-28T00:00:00.000Z')
  })

  it('is the same day on a Monday', () => {
    const d = startOfWeek(new Date('2026-09-14T12:00:00Z'), 'UTC')
    expect(d.toISOString()).toBe('2026-09-14T00:00:00.000Z')
  })
})

describe('startOfMonth and startOfQuarter', () => {
  it('drop to the first at midnight', () => {
    expect(startOfMonth(new Date('2026-09-17T10:00:00Z'), 'UTC').toISOString()).toBe(
      '2026-09-01T00:00:00.000Z',
    )
    expect(startOfQuarter(new Date('2026-11-15T10:00:00Z'), 'UTC').toISOString()).toBe(
      '2026-10-01T00:00:00.000Z',
    )
    expect(startOfQuarter(new Date('2026-01-01T00:00:00Z'), 'UTC').toISOString()).toBe(
      '2026-01-01T00:00:00.000Z',
    )
  })
})

describe('daysBetween', () => {
  it('counts calendar days, ignoring the time of day', () => {
    const a = new Date('2026-09-17T23:59:00Z')
    const b = new Date('2026-09-18T00:01:00Z')
    expect(daysBetween(a, b, 'UTC')).toBe(1)
    expect(daysBetween(b, a, 'UTC')).toBe(-1)
    expect(daysBetween(a, a, 'UTC')).toBe(0)
  })

  it('reckons the days in the given zone', () => {
    // Both instants are the 17th in UTC; in Sydney the second is the 18th.
    const a = new Date('2026-09-17T01:00:00Z')
    const b = new Date('2026-09-17T15:00:00Z')
    expect(daysBetween(a, b, 'UTC')).toBe(0)
    expect(daysBetween(a, b, SYDNEY)).toBe(1)
  })
})

describe('calendar date strings', () => {
  it('round-trip through the zone', () => {
    const d = fromCalendarDateString('2026-09-17', SYDNEY)
    expect(d?.toISOString()).toBe('2026-09-16T14:00:00.000Z')
    expect(toCalendarDateString(d!, SYDNEY)).toBe('2026-09-17')
  })

  it('truncate a datetime to its date part', () => {
    expect(fromCalendarDateString('2026-09-17T23:00:00', 'UTC')?.toISOString()).toBe(
      '2026-09-17T00:00:00.000Z',
    )
  })

  it('reject dates that do not exist', () => {
    expect(fromCalendarDateString('2026-02-30', 'UTC')).toBeNull()
    expect(fromCalendarDateString('not a date', 'UTC')).toBeNull()
    expect(fromCalendarDateString('', 'UTC')).toBeNull()
  })
})

describe('the browser-zone default', () => {
  it('is what a local-midnight Date reads back as', () => {
    const local = new Date(2026, 8, 17)
    expect(getLocalTimeZone()).toBeTruthy()
    expect(toCalendarDateString(local)).toBe('2026-09-17')
    expect(startOfDay(local).getTime()).toBe(local.getTime())
  })
})
