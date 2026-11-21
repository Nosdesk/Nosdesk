/**
 * The ticket heatmap's days are calendar days in the viewer's zone, and a
 * day's click-through lists the tickets of that same day. Every case
 * names its zone so the result is the same on any machine.
 */
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { resolveDashboardViewFilter } from '@nosdesk/core/sync/views/builtinViews'
import { buildPredicate } from '@nosdesk/core/sync/views/filter'
import type { CardData } from '@nosdesk/core/sync/views/types'
import {
  heatmapDayKeys,
  heatmapDayRoute,
  heatmapDayWindow,
  shiftDayKey,
  ticketHeatmapDay,
  weekdayOfDayKey,
} from '@/utils/ticketHeatmap'

// UTC+11 in November (daylight time).
const SYDNEY = 'Australia/Sydney'

const closed = (closed_at: string | null) => ({ closed_at, modified: '2026-11-01T00:00:00Z' })

describe('ticketHeatmapDay', () => {
  it("counts a closed ticket on the viewer's day, not the UTC date", () => {
    // 09:30 on the 21st in Sydney.
    expect(ticketHeatmapDay(closed('2026-11-20T22:30:00Z'), 'completed', SYDNEY)).toBe('2026-11-21')
    expect(ticketHeatmapDay(closed('2026-11-20T22:30:00Z'), 'completed', 'UTC')).toBe('2026-11-20')
  })

  it('changes day at local midnight', () => {
    expect(ticketHeatmapDay(closed('2026-11-21T12:59:00Z'), 'completed', SYDNEY)).toBe('2026-11-21')
    expect(ticketHeatmapDay(closed('2026-11-21T13:01:00Z'), 'completed', SYDNEY)).toBe('2026-11-22')
  })

  it('reads a zoneless server timestamp as UTC', () => {
    expect(ticketHeatmapDay(closed('2026-11-20T22:30:00.123456'), 'completed', SYDNEY)).toBe('2026-11-21')
  })

  it('falls back to modified when a closed ticket has no closed_at', () => {
    const ticket = { closed_at: null, modified: '2026-11-20T22:30:00Z' }
    expect(ticketHeatmapDay(ticket, 'completed', SYDNEY)).toBe('2026-11-21')
  })

  it('reads modified in active mode', () => {
    const ticket = { closed_at: '2026-10-01T00:00:00Z', modified: '2026-11-21T13:01:00Z' }
    expect(ticketHeatmapDay(ticket, 'active', SYDNEY)).toBe('2026-11-22')
  })
})

describe('heatmapDayKeys', () => {
  it("ends on the viewer's today", () => {
    // 07:00 on the 21st in Sydney, still the 20th in UTC.
    const now = new Date('2026-11-20T20:00:00Z')
    const sydney = heatmapDayKeys(now, SYDNEY)
    expect(sydney).toHaveLength(365)
    expect(sydney[364]).toBe('2026-11-21')
    expect(sydney[0]).toBe('2025-11-22')
    expect(heatmapDayKeys(now, 'UTC')[364]).toBe('2026-11-20')
  })
})

describe('day key arithmetic', () => {
  it('steps across months and years', () => {
    expect(shiftDayKey('2026-12-31', 1)).toBe('2027-01-01')
    expect(shiftDayKey('2026-03-01', -1)).toBe('2026-02-28')
  })

  it('names the weekday of the calendar day', () => {
    expect(weekdayOfDayKey('2026-11-22')).toBe(0)
    expect(weekdayOfDayKey('2026-11-21')).toBe(6)
  })
})

describe('heatmapDayWindow', () => {
  it('runs from local midnight to the next local midnight', () => {
    expect(heatmapDayWindow('2026-11-21', SYDNEY)).toEqual({
      from: '2026-11-20T13:00:00.000Z',
      to: '2026-11-21T13:00:00.000Z',
    })
  })

  it('is 25 hours on the day daylight time ends', () => {
    expect(heatmapDayWindow('2026-04-05', SYDNEY)).toEqual({
      from: '2026-04-04T13:00:00.000Z',
      to: '2026-04-05T14:00:00.000Z',
    })
  })
})

describe('heatmapDayRoute', () => {
  // The list filters in the browser, so its zone is the process zone:
  // pin it to Sydney, where reading a zoneless UTC stamp as local time
  // would show.
  const runnerTz = process.env.TZ
  beforeAll(() => {
    process.env.TZ = SYDNEY
  })
  afterAll(() => {
    if (runnerTz === undefined) delete process.env.TZ
    else process.env.TZ = runnerTz
  })

  const card = (closed_at: string): CardData => ({
    id: 7,
    number: 7,
    title: 'Printer on fire',
    workflow_state: { id: 2, name: 'Done', category: 'done', color: 'gray' },
    priority: 'none',
    created_at: '2026-11-19T00:00:00Z',
    updated_at: closed_at,
    last_activity_at: closed_at,
    closed_at,
  })

  it('lists the tickets closed on that local day', () => {
    expect(new Date(2026, 10, 21).getTimezoneOffset()).toBe(-660)
    const route = heatmapDayRoute('2026-11-21', 'completed', SYDNEY) as {
      path: string
      query: Record<string, string>
    }
    expect(route.path).toBe('/tickets')
    expect(route.query.view).toBe('dashboard-resolved')

    const filter = resolveDashboardViewFilter(route.query.view, route.query.from, route.query.to)
    const shows = buildPredicate(filter, { currentUserUuid: null })
    // The pool carries the server's zoneless UTC timestamps.
    expect(shows(card('2026-11-20T22:30:00'))).toBe(true)
    expect(shows(card('2026-11-21T12:59:00'))).toBe(true)
    expect(shows(card('2026-11-21T13:01:00'))).toBe(false)
    expect(shows(card('2026-11-20T12:59:00'))).toBe(false)
  })

  it('lists the tickets created on that day in active mode', () => {
    const route = heatmapDayRoute('2026-11-21', 'active', SYDNEY) as {
      query: Record<string, string>
    }
    expect(route.query).toEqual({
      view: 'dashboard-created',
      from: '2026-11-20T13:00:00.000Z',
      to: '2026-11-21T13:00:00.000Z',
    })
  })
})
