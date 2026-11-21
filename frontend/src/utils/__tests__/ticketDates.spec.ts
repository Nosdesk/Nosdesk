/**
 * `@nosdesk/core/utils/ticketDates`: a due or start date is the day as
 * written, a due day ends at the next midnight in the planning zone,
 * and the due buckets split around today in that zone.
 */
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { setDateConfig } from '@nosdesk/core/utils/dateUtils'
import {
  dueBucket,
  dueChipLabel,
  dueDay,
  dueInstant,
  floatingDay,
  formatCompactDay,
  isOverdue,
  planDay,
  todayIn,
} from '@nosdesk/core/utils/ticketDates'
import { buildPredicate } from '@nosdesk/core/sync/views/filter'
import type { CardData } from '@nosdesk/core/sync/views/types'

const SYDNEY = 'Australia/Sydney'
const LOS_ANGELES = 'America/Los_Angeles'

beforeEach(() => setDateConfig({ defaultLocale: 'en-US', defaultTimezone: 'UTC' }))
afterEach(() => setDateConfig({ defaultLocale: 'en-US', defaultTimezone: 'UTC' }))

describe('reading a stored day', () => {
  it('takes the date part as written, whatever follows it', () => {
    expect(floatingDay('2026-11-21')).toBe('2026-11-21')
    expect(floatingDay('2026-11-21T00:00:00')).toBe('2026-11-21')
    expect(floatingDay('2026-11-21T00:00:00Z')).toBe('2026-11-21')
    // Written with an offset: still the 21st, never shifted to UTC.
    expect(floatingDay('2026-11-21T00:00:00+11:00')).toBe('2026-11-21')
    expect(floatingDay('2026-11-21T23:30:00-08:00')).toBe('2026-11-21')
  })

  it('is null for nothing, or for something that is not a day', () => {
    expect(floatingDay(null)).toBeNull()
    expect(floatingDay('')).toBeNull()
    expect(floatingDay('soon')).toBeNull()
    expect(floatingDay('2026-02-30T00:00:00')).toBeNull()
  })

  it('plans on the start day when there is one, else the due day', () => {
    expect(planDay({ start_date: '2026-11-24T00:00:00', due_date: '2026-11-27T00:00:00' })).toBe('2026-11-24')
    expect(planDay({ start_date: null, due_date: '2026-11-27T00:00:00Z' })).toBe('2026-11-27')
    expect(planDay({})).toBeNull()
  })

  it('formats the day itself in any zone, so west of UTC is not the day before', () => {
    setDateConfig({ defaultTimezone: LOS_ANGELES })
    expect(formatCompactDay(dueDay({ due_date: '2026-11-21T00:00:00Z' })!, '2026-11-20')).toBe('Nov 21')
    setDateConfig({ defaultTimezone: SYDNEY })
    expect(formatCompactDay(dueDay({ due_date: '2026-11-21T00:00:00' })!, '2026-11-20')).toBe('Nov 21')
    expect(formatCompactDay('2027-01-04', '2026-11-20')).toBe('Jan 4, 2027')
  })
})

describe('the due instant', () => {
  // The same three cases as the backend's `ticket_dates::due_instant`.
  it('is the next midnight in the zone', () => {
    expect(dueInstant('2026-11-21', SYDNEY)?.toISOString()).toBe('2026-11-21T13:00:00.000Z')
    expect(dueInstant('2026-11-21', 'America/New_York')?.toISOString()).toBe('2026-11-22T05:00:00.000Z')
    expect(dueInstant('2026-11-21', LOS_ANGELES)?.toISOString()).toBe('2026-11-22T08:00:00.000Z')
    expect(dueInstant('2026-11-21', 'UTC')?.toISOString()).toBe('2026-11-22T00:00:00.000Z')
  })

  it('starts after the gap in a zone that skips midnight', () => {
    // Santiago moved its clocks from 00:00 to 01:00 on 2023-09-03.
    expect(dueInstant('2023-09-02', 'America/Santiago')?.toISOString()).toBe('2023-09-03T04:00:00.000Z')
  })
})

describe('overdue', () => {
  const dueSat = { due_date: '2026-11-21T00:00:00' }

  it('waits for the end of the due day in Sydney', () => {
    // 23:59 and 00:00 Sydney time (UTC+11) either side of the 21st's end.
    expect(isOverdue(dueSat, new Date('2026-11-21T12:59:00Z'), SYDNEY)).toBe(false)
    expect(isOverdue(dueSat, new Date('2026-11-21T13:00:00Z'), SYDNEY)).toBe(true)
  })

  it('waits for the end of the due day in Los Angeles', () => {
    // A minute past midnight UTC on the 22nd is still the 21st in LA.
    expect(isOverdue(dueSat, new Date('2026-11-22T00:01:00Z'), LOS_ANGELES)).toBe(false)
    expect(isOverdue(dueSat, new Date('2026-11-22T07:59:00Z'), LOS_ANGELES)).toBe(false)
    expect(isOverdue(dueSat, new Date('2026-11-22T08:00:00Z'), LOS_ANGELES)).toBe(true)
  })

  it('reads a value written with an offset as its written day', () => {
    // The 21st written from UTC+11; it is the 21st, not the 20th in UTC.
    const written = { due_date: '2026-11-21T00:00:00+11:00' }
    expect(isOverdue(written, new Date('2026-11-21T23:00:00Z'), 'UTC')).toBe(false)
    expect(isOverdue(written, new Date('2026-11-22T00:00:00Z'), 'UTC')).toBe(true)
  })

  it('never applies without a due date', () => {
    expect(isOverdue({ due_date: null }, new Date('2030-01-01T00:00:00Z'), SYDNEY)).toBe(false)
  })

  it('drives the overdue quick filter, so a ticket due today is not overdue', () => {
    const now = new Date()
    setDateConfig({ defaultTimezone: SYDNEY })
    const today = todayIn(SYDNEY, now)
    const card = (due: string) => ({ due_date: `${due}T00:00:00` }) as unknown as CardData
    const yesterday = new Date(now.getTime() - 86_400_000)
    const overdue = buildPredicate(
      {
        predicate: { combinator: 'AND', children: [] },
        quick_filters: ['overdue'],
        scope: { project_ids: 'all', cycle_ids: 'all', group_ids: 'all', include_archived: false },
      },
      { currentUserUuid: null },
    )
    expect(overdue(card(today))).toBe(false)
    expect(overdue(card(todayIn(SYDNEY, yesterday)))).toBe(true)
  })
})

describe('due buckets', () => {
  const on = (day: string | null) => ({ due_date: day ? `${day}T00:00:00` : null })

  it('splits around today, with "this week" running to Sunday', () => {
    // Wednesday 18 November 2026.
    const today = '2026-11-18'
    expect(dueBucket(on('2026-11-17'), today)).toBe('overdue')
    expect(dueBucket(on('2026-11-18'), today)).toBe('today')
    expect(dueBucket(on('2026-11-19'), today)).toBe('tomorrow')
    expect(dueBucket(on('2026-11-20'), today)).toBe('this-week')
    expect(dueBucket(on('2026-11-22'), today)).toBe('this-week')
    expect(dueBucket(on('2026-11-23'), today)).toBe('later')
    expect(dueBucket(on(null), today)).toBe('none')
  })

  it('has no "this week" left on a Saturday', () => {
    expect(dueBucket(on('2026-11-22'), '2026-11-21')).toBe('tomorrow')
    expect(dueBucket(on('2026-11-23'), '2026-11-21')).toBe('later')
  })

  it('moves a job from Today to Overdue at midnight in the zone', () => {
    const job = on('2026-11-21')
    // Sydney: 23:59 on the 21st, then 00:00 on the 22nd.
    expect(dueBucket(job, todayIn(SYDNEY, new Date('2026-11-21T12:59:00Z')))).toBe('today')
    expect(dueBucket(job, todayIn(SYDNEY, new Date('2026-11-21T13:00:00Z')))).toBe('overdue')
    // Los Angeles (UTC-8): 23:59 on the 20th the job is tomorrow's,
    // all through the 21st it is today's, and at midnight overdue.
    expect(dueBucket(job, todayIn(LOS_ANGELES, new Date('2026-11-21T07:59:00Z')))).toBe('tomorrow')
    expect(dueBucket(job, todayIn(LOS_ANGELES, new Date('2026-11-21T13:00:00Z')))).toBe('today')
    expect(dueBucket(job, todayIn(LOS_ANGELES, new Date('2026-11-22T07:59:00Z')))).toBe('today')
    expect(dueBucket(job, todayIn(LOS_ANGELES, new Date('2026-11-22T08:00:00Z')))).toBe('overdue')
  })

  it('agrees with overdue at the same moment and zone', () => {
    const job = on('2026-11-21')
    for (const tz of [SYDNEY, LOS_ANGELES]) {
      for (const iso of ['2026-11-21T12:59:00Z', '2026-11-21T13:00:00Z', '2026-11-22T08:00:00Z']) {
        const now = new Date(iso)
        expect(dueBucket(job, todayIn(tz, now)) === 'overdue').toBe(isOverdue(job, now, tz))
      }
    }
  })
})

describe('the due chip', () => {
  const words = { today: 'Today', tomorrow: 'Tomorrow' }

  it('says Today, Tomorrow, a weekday within the week, else the date', () => {
    setDateConfig({ defaultLocale: 'en-AU' })
    expect(dueChipLabel('2026-11-21', '2026-11-21', words)).toBe('Today')
    expect(dueChipLabel('2026-11-22', '2026-11-21', words)).toBe('Tomorrow')
    expect(dueChipLabel('2026-11-27', '2026-11-21', words)).toBe('Fri 27')
    expect(dueChipLabel('2026-11-20', '2026-11-21', words)).toBe('Fri 20')
    expect(dueChipLabel('2026-12-24', '2026-11-21', words)).toBe('24 Dec')
  })
})
