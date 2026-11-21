/**
 * The Completed work heatmap: Done tickets only, on the day of their
 * closed_at in the viewer's zone, credited to the assignee (or whoever
 * closed an unassigned one), shaded against the person's own year, and a
 * day opens a list of exactly the jobs its cell counted. Every case names
 * its zone so the result is the same on any machine.
 */
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { resolveDashboardViewFilter } from '@nosdesk/core/sync/views/builtinViews'
import { buildPredicate } from '@nosdesk/core/sync/views/filter'
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow'
import { setDateConfig } from '@nosdesk/core/utils/dateUtils'
import enUS from '../../../../i18n/locales/en-US/main.ftl?raw'
import frFR from '../../../../i18n/locales/fr-FR/main.ftl?raw'
import nlNL from '../../../../i18n/locales/nl-NL/main.ftl?raw'
import type { SyncTicket } from '@/sync/stores/tickets'
import { toCardData } from '@/sync/views/cardData'
import { dashboardDrillDownScopeFromQuery } from '@/utils/dashboardTicketDrillDown'
import {
  FIXED_SHADE_BOUNDARIES,
  buildCompletedWork,
  completedDay,
  creditedTo,
  hasAssignedWork,
  heatmapScopeStorageKey,
  loadHeatmapScope,
  saveHeatmapScope,
  heatmapDayKeys,
  heatmapDayRoute,
  heatmapDayTitle,
  heatmapDayWindow,
  jobWeight,
  shadeBoundaries,
  shadeLevel,
  shiftDayKey,
  weekdayOfDayKey,
} from '@/utils/ticketHeatmap'

// UTC+11 in November (daylight time).
const SYDNEY = 'Australia/Sydney'
const ME = 'me-uuid'
const OTHER = 'other-uuid'
/** 07:00 on Sunday 22 November in Sydney. */
const NOW = new Date('2026-11-21T20:00:00Z')

let nextId = 1

/** A pool row as bootstrap sends it: zoneless UTC timestamps. */
function row(
  category: WorkflowStateCategory,
  closed_at: string | null,
  extra: Partial<SyncTicket> = {},
): SyncTicket {
  const id = nextId++
  return {
    id,
    number: id,
    uuid: `t-${id}`,
    title: `Job ${id}`,
    workflow_state: { id: 1, name: category, category, color: 'gray' },
    workflow_state_id: 1,
    priority: 'none',
    spam_suspected: false,
    requester_uuid: null,
    assignee_uuid: ME,
    category_id: null,
    triage_state: null,
    due_date: null,
    created_at: '2026-11-01T00:00:00',
    updated_at: '2026-11-21T00:00:00',
    last_activity_at: '2026-11-21T00:00:00',
    closed_at,
    closed_by: null,
    ...extra,
  }
}

describe('completedDay', () => {
  it("counts a Done ticket on the viewer's day, not the UTC date", () => {
    // 09:30 on the 21st in Sydney.
    expect(completedDay(row('done', '2026-11-20T22:30:00Z'), SYDNEY)).toBe('2026-11-21')
    expect(completedDay(row('done', '2026-11-20T22:30:00Z'), 'UTC')).toBe('2026-11-20')
  })

  it('changes day at local midnight', () => {
    expect(completedDay(row('done', '2026-11-21T12:59:00Z'), SYDNEY)).toBe('2026-11-21')
    expect(completedDay(row('done', '2026-11-21T13:01:00Z'), SYDNEY)).toBe('2026-11-22')
  })

  it('reads a zoneless pool timestamp as UTC', () => {
    expect(completedDay(row('done', '2026-11-20T22:30:00.123456'), SYDNEY)).toBe('2026-11-21')
  })

  it('counts Done only: cancelled and merged are not work done', () => {
    expect(completedDay(row('cancelled', '2026-11-20T22:30:00'), SYDNEY)).toBeNull()
    expect(completedDay(row('merged', '2026-11-20T22:30:00'), SYDNEY)).toBeNull()
    expect(completedDay(row('active', null), SYDNEY)).toBeNull()
  })

  it('has no fallback when closed_at is missing', () => {
    const ticket = row('done', null, { updated_at: '2026-11-20T22:30:00' })
    expect(completedDay(ticket, SYDNEY)).toBeNull()
  })
})

describe('credit', () => {
  it('goes to the assignee, or to whoever closed an unassigned ticket', () => {
    expect(creditedTo(row('done', null, { assignee_uuid: OTHER, closed_by: ME }))).toBe(OTHER)
    expect(creditedTo(row('done', null, { assignee_uuid: null, closed_by: ME }))).toBe(ME)
    expect(creditedTo(row('done', null, { assignee_uuid: null, closed_by: null }))).toBeNull()
  })

  it('makes Mine the default only for someone with assigned work', () => {
    const pool = [row('active', null, { assignee_uuid: OTHER }), row('done', null, { assignee_uuid: ME })]
    expect(hasAssignedWork(pool, ME)).toBe(true)
    expect(hasAssignedWork(pool.slice(0, 1), ME)).toBe(false)
    expect(hasAssignedWork(pool, null)).toBe(false)
  })
})

describe('the remembered Mine / Everyone choice', () => {
  afterEach(() => localStorage.clear())

  it('is kept per workspace and user on this device', () => {
    const mine = heatmapScopeStorageKey(1, ME)
    saveHeatmapScope(localStorage, mine, 'everyone')
    expect(loadHeatmapScope(localStorage, mine)).toBe('everyone')
    expect(loadHeatmapScope(localStorage, heatmapScopeStorageKey(2, ME))).toBeNull()
    expect(loadHeatmapScope(localStorage, heatmapScopeStorageKey(1, OTHER))).toBeNull()
  })

  it('falls back to the default rule when nothing usable is stored', () => {
    const key = heatmapScopeStorageKey(1, ME)
    expect(loadHeatmapScope(localStorage, key)).toBeNull()
    localStorage.setItem(key!, 'someone')
    expect(loadHeatmapScope(localStorage, key)).toBeNull()
    expect(loadHeatmapScope(undefined, key)).toBeNull()
  })

  it('is not stored without a workspace and a user', () => {
    expect(heatmapScopeStorageKey(null, ME)).toBeNull()
    expect(heatmapScopeStorageKey(1, null)).toBeNull()
    saveHeatmapScope(localStorage, null, 'mine')
    expect(localStorage.length).toBe(0)
  })

  it('survives a storage that refuses access', () => {
    const blocked = {
      getItem: () => {
        throw new Error('blocked')
      },
      setItem: () => {
        throw new Error('blocked')
      },
    } as unknown as Storage
    expect(() => saveHeatmapScope(blocked, 'k', 'mine')).not.toThrow()
    expect(loadHeatmapScope(blocked, 'k')).toBeNull()
  })
})

describe('jobWeight', () => {
  it('weighs an unsized job as medium', () => {
    expect(jobWeight(undefined)).toBe(2)
    expect(jobWeight(null)).toBe(2)
  })

  it('maps planned minutes to small 1, medium 2, big 4', () => {
    expect(jobWeight(30)).toBe(1)
    expect(jobWeight(60)).toBe(1)
    expect(jobWeight(120)).toBe(2)
    expect(jobWeight(240)).toBe(2)
    expect(jobWeight(360)).toBe(4)
  })
})

describe('shade', () => {
  it('uses 2, 4 and 8 with fewer than eight active days', () => {
    expect(shadeBoundaries([0, 2, 4, 6, 0, 2, 2, 2])).toEqual(FIXED_SHADE_BOUNDARIES)
    // One, two, three to four, and five or more medium jobs.
    expect([2, 4, 6, 8, 10].map((w) => shadeLevel(w, FIXED_SHADE_BOUNDARIES))).toEqual([1, 2, 3, 3, 4])
  })

  it("uses the quartiles of the person's non-zero days from eight on", () => {
    const year = [0, 0, 2, 4, 6, 8, 10, 12, 14, 16, 0]
    expect(shadeBoundaries(year)).toEqual([5.5, 9, 12.5])
    expect([2, 6, 10, 14].map((w) => shadeLevel(w, shadeBoundaries(year)))).toEqual([1, 2, 3, 4])
  })

  it('a busy year no longer saturates: the darkest shade is a big day for this person', () => {
    const busy = Array.from({ length: 200 }, (_, i) => 10 + (i % 10) * 2)
    const levels = busy.map((w) => shadeLevel(w, shadeBoundaries(busy)))
    expect(new Set(levels)).toEqual(new Set([1, 2, 3, 4]))
  })

  it('never shades a day with work as empty, even when every day is the same', () => {
    const flat = Array(20).fill(2)
    expect(shadeLevel(2, shadeBoundaries(flat))).toBe(1)
    expect(shadeLevel(0, shadeBoundaries(flat))).toBe(0)
  })
})

describe('buildCompletedWork', () => {
  const pool = [
    row('done', '2026-11-20T22:30:00'), // mine, Sat 21 Nov in Sydney
    row('done', '2026-11-21T12:59:00', { assignee_uuid: null, closed_by: ME }), // mine by closing it
    row('done', '2026-11-21T02:00:00', { assignee_uuid: OTHER, closed_by: ME }), // someone else's
    row('cancelled', '2026-11-21T02:00:00'),
    row('merged', '2026-11-21T02:00:00'),
    row('done', null, { updated_at: '2026-11-21T02:00:00' }),
    row('done', '2026-11-21T13:01:00'), // mine, Sun 22 Nov
  ]
  const day = (scope: 'mine' | 'everyone', key: string) =>
    buildCompletedWork(pool, { now: NOW, tz: SYDNEY, scope, me: ME }).find((d) => d.date === key)!

  it('ends on the local today with a year of days', () => {
    const days = buildCompletedWork(pool, { now: NOW, tz: SYDNEY, scope: 'mine', me: ME })
    expect(days).toHaveLength(365)
    expect(days[364].date).toBe('2026-11-22')
  })

  it("counts the viewer's credited Done tickets under Mine", () => {
    expect(day('mine', '2026-11-21').count).toBe(2)
    expect(day('mine', '2026-11-21').weight).toBe(4)
    expect(day('mine', '2026-11-22').count).toBe(1)
  })

  it("adds everyone's under Everyone, still Done only", () => {
    expect(day('everyone', '2026-11-21').count).toBe(3)
  })

  it('counts nothing under Mine for a viewer with no identity', () => {
    const days = buildCompletedWork(pool, { now: NOW, tz: SYDNEY, scope: 'mine', me: null })
    expect(days.every((d) => d.count === 0)).toBe(true)
  })
})

describe('heatmapDayKeys and day keys', () => {
  it("ends on the viewer's today", () => {
    // 07:00 on the 21st in Sydney, still the 20th in UTC.
    const now = new Date('2026-11-20T20:00:00Z')
    expect(heatmapDayKeys(now, SYDNEY)[364]).toBe('2026-11-21')
    expect(heatmapDayKeys(now, SYDNEY)[0]).toBe('2025-11-22')
    expect(heatmapDayKeys(now, 'UTC')[364]).toBe('2026-11-20')
  })

  it('steps across months and years, and names the weekday', () => {
    expect(shiftDayKey('2026-12-31', 1)).toBe('2027-01-01')
    expect(shiftDayKey('2026-03-01', -1)).toBe('2026-02-28')
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

describe('heatmapDayTitle', () => {
  function translator(source: string, locale: string) {
    const bundle = new FluentBundle(locale, { useIsolating: false })
    bundle.addResource(new FluentResource(source))
    return (key: string, args: Record<string, string | number>) => {
      const msg = bundle.getMessage(key)
      return msg?.value ? bundle.formatPattern(msg.value, args) : `{${key}}`
    }
  }
  afterEach(() => setDateConfig({ defaultLocale: 'en-US', defaultTimezone: 'UTC' }))

  it('reads "Tue 17 Nov: 5 jobs done", in the order the locale writes dates', () => {
    const t = translator(enUS, 'en-US')
    setDateConfig({ defaultLocale: 'en-AU', defaultTimezone: SYDNEY })
    expect(heatmapDayTitle({ date: '2026-11-17', count: 5 }, t)).toMatch(/^Tue,? 17 Nov: 5 jobs done$/)
    expect(heatmapDayTitle({ date: '2026-11-17', count: 1 }, t)).toMatch(/: 1 job done$/)
    expect(heatmapDayTitle({ date: '2026-11-17', count: 0 }, t)).toMatch(/: no jobs done$/)
    setDateConfig({ defaultLocale: 'en-US', defaultTimezone: 'America/Los_Angeles' })
    expect(heatmapDayTitle({ date: '2026-11-17', count: 5 }, t)).toMatch(/^Tue, Nov 17: 5 jobs done$/)
  })

  it('has the message in fr-FR and nl-NL', () => {
    for (const [source, locale] of [[frFR, 'fr-FR'], [nlNL, 'nl-NL']] as const) {
      setDateConfig({ defaultLocale: locale })
      const title = heatmapDayTitle({ date: '2026-11-17', count: 5 }, translator(source, locale))
      expect(title).toContain('17')
      expect(title).toContain('5 ')
    }
  })
})

describe('a day opens the jobs its cell counted', () => {
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

  const pool = [
    row('done', '2026-11-20T22:30:00'),
    row('done', '2026-11-21T12:59:00', { assignee_uuid: null, closed_by: ME }),
    row('done', '2026-11-21T02:00:00', { assignee_uuid: OTHER, closed_by: ME }),
    row('done', '2026-11-21T03:00:00', { assignee_uuid: null, closed_by: OTHER }),
    row('cancelled', '2026-11-21T02:00:00'),
    row('merged', '2026-11-21T02:00:00'),
    row('done', '2026-11-21T13:01:00'),
    row('done', '2026-11-20T12:59:00'),
  ]

  function listed(scope: 'mine' | 'everyone'): number[] {
    const route = heatmapDayRoute('2026-11-21', SYDNEY, scope, ME) as {
      path: string
      query: Record<string, string>
    }
    expect(route.path).toBe('/tickets')
    expect(route.query.view).toBe('dashboard-resolved')
    const { view, from, to } = route.query
    const filter = resolveDashboardViewFilter(view, from, to, dashboardDrillDownScopeFromQuery(route.query))
    const shows = buildPredicate(filter, { currentUserUuid: ME })
    return pool.filter((t) => shows(toCardData(t)!)).map((t) => t.id)
  }

  function counted(scope: 'mine' | 'everyone'): number[] {
    const days = buildCompletedWork(pool, { now: NOW, tz: SYDNEY, scope, me: ME })
    return days.find((d) => d.date === '2026-11-21')!.tickets.map((t) => t.id)
  }

  it('under Mine', () => {
    expect(new Date(2026, 10, 21).getTimezoneOffset()).toBe(-660)
    expect(listed('mine')).toEqual(counted('mine'))
    expect(counted('mine')).toEqual([pool[0].id, pool[1].id])
  })

  it('under Everyone', () => {
    expect(listed('everyone')).toEqual(counted('everyone'))
    expect(counted('everyone')).toEqual([pool[0].id, pool[1].id, pool[2].id, pool[3].id])
  })

  it("leaves the Resolved KPI's own list as it was: cancelled stays in, merged out", () => {
    const { from, to } = heatmapDayWindow('2026-11-21', SYDNEY)
    const shows = buildPredicate(resolveDashboardViewFilter('dashboard-resolved', from, to), {
      currentUserUuid: ME,
    })
    expect(shows(toCardData(pool[4])!)).toBe(true)
    expect(shows(toCardData(pool[5])!)).toBe(false)
  })
})
