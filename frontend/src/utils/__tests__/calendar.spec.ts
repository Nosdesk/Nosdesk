import { describe, expect, it } from 'vitest'
import {
  agendaDays,
  agendaRange,
  agendaWindow,
  anchorDay,
  cellLines,
  dayNumber,
  dayTitleParts,
  earliestLate,
  formatCellTime,
  isLate,
  jobsByDay,
  jobsOn,
  monthWeeks,
  movePatch,
  shiftDayByMonths,
  shiftMonth,
  wallClock,
  windowToReveal,
} from '@nosdesk/core/sync/views/calendar'
import { planPatchEffects } from '@nosdesk/core/utils/ticketPlan'
import { planDay } from '@nosdesk/core/utils/ticketDates'
import type { CardData, Priority } from '@nosdesk/core/sync/views/types'

const TODAY = '2026-11-25'
const CLOCK = { day: TODAY, time: '10:00' }

function job(id: number, plan: Partial<CardData> = {}, priority: Priority = 'none'): CardData {
  return {
    id,
    number: 4800 + id,
    title: `Job ${id}`,
    workflow_state: { id: 2, name: 'Open', category: 'backlog', color: 'gray' },
    priority,
    due_date: `${TODAY}T00:00:00`,
    created_at: `2026-11-${String(id).padStart(2, '0')}T00:00:00Z`,
    updated_at: '2026-11-01T00:00:00Z',
    last_activity_at: '2026-11-01T00:00:00Z',
    ...plan,
  }
}

const ids = (cards: readonly CardData[]) => cards.map((c) => c.id)

describe('the month', () => {
  it('shows whole weeks from the one holding the 1st, starting where the locale starts the week', () => {
    // 1 November 2026 is a Sunday.
    const monday = monthWeeks('2026-11', 'en-AU')
    expect(monday).toHaveLength(6)
    expect(monday[0][0]).toBe('2026-10-26')
    expect(monday[5][6]).toBe('2026-12-06')

    const sunday = monthWeeks('2026-11', 'en-US')
    expect(sunday).toHaveLength(5)
    expect(sunday[0][0]).toBe('2026-11-01')
    expect(sunday[4][6]).toBe('2026-12-05')
  })

  it('takes four rows when the month fills them exactly, and never an empty row', () => {
    // February 2026 runs Sunday the 1st to Saturday the 28th.
    const weeks = monthWeeks('2026-02', 'en-US')
    expect(weeks).toHaveLength(4)
    expect(weeks[3][6]).toBe('2026-02-28')
    for (const week of monthWeeks('2026-05', 'fr-FR')) {
      expect(week.some((day) => day.startsWith('2026-05'))).toBe(true)
    }
  })

  it('turns months and keeps a day inside the month it lands in', () => {
    expect(shiftMonth('2026-12', 1)).toBe('2027-01')
    expect(shiftMonth('2026-01', -1)).toBe('2025-12')
    expect(shiftDayByMonths('2026-01-31', 1)).toBe('2026-02-28')
  })

  it('names the first of a neighbouring month in its cell', () => {
    expect(dayNumber('2026-12-01', '2026-11', 'en-AU')).toBe('1 Dec')
    expect(dayNumber('2026-12-02', '2026-11', 'en-AU')).toBe('2')
    expect(dayNumber('2026-11-01', '2026-11', 'en-AU')).toBe('1')
  })

  it('writes a cell time short, on the locale clock', () => {
    expect(formatCellTime('09:00:00', 'en-AU')).toBe('9am')
    expect(formatCellTime('14:30', 'en-US')).toBe('2:30pm')
    expect(formatCellTime('14:30', 'en-GB')).toBe('14:30')
  })

  it('marks the day of the month in a day title', () => {
    const parts = dayTitleParts('2026-11-25', 'en-AU')
    expect(parts.map((p) => p.text).join('')).toBe('Wednesday 25 November')
    expect(parts.filter((p) => p.number).map((p) => p.text)).toEqual(['25'])
  })

  it('fills a cell with the lines it holds and counts the rest on the last line', () => {
    expect(cellLines([1, 2, 3], 4)).toEqual({ shown: [1, 2, 3], more: 0 })
    expect(cellLines([1, 2, 3, 4], 4)).toEqual({ shown: [1, 2, 3, 4], more: 0 })
    expect(cellLines([1, 2, 3, 4, 5, 6], 4)).toEqual({ shown: [1, 2, 3], more: 3 })
    expect(cellLines([1, 2], 1)).toEqual({ shown: [], more: 2 })
  })
})

describe('the plan day', () => {
  it('places a job on its start date when it has one, else its due date', () => {
    expect(anchorDay(job(1, { start_date: '2026-11-24T00:00:00' }), 'due_date')).toBe('2026-11-24')
    expect(anchorDay(job(2), 'due_date')).toBe(TODAY)
    expect(anchorDay(job(3, { due_date: null }), 'due_date')).toBeNull()
    // A stray offset does not move a floating day.
    expect(anchorDay(job(4, { due_date: '2026-11-21T09:30:00+10:00' }), 'due_date')).toBe('2026-11-21')
  })

  it('places a moment on the day it fell on in the planning timezone', () => {
    const card = job(5, { created_at: '2026-11-24T22:30:00Z' })
    expect(anchorDay(card, 'created_at', 'Australia/Sydney')).toBe('2026-11-25')
    expect(anchorDay(card, 'created_at', 'UTC')).toBe('2026-11-24')
  })

  it('reads a day with timed jobs first by time, then the day order', () => {
    const cards = [
      job(1, {}, 'low'),
      job(2, { due_time: '14:00:00' }),
      job(3, { rank: 'V' }),
      job(4, { due_time: '09:00:00' }),
      job(5, {}, 'urgent'),
      // Booked on Tuesday against Wednesday's deadline: on Tuesday.
      job(6, { start_date: '2026-11-24T00:00:00', start_time: '08:00:00' }),
    ]
    const byDay = jobsByDay(cards, 'due_date')
    const today = jobsOn(byDay, TODAY)
    expect(ids(today.jobs)).toEqual([4, 2, 3, 5, 1])
    expect(today.timed).toBe(2)
    expect(ids(jobsOn(byDay, '2026-11-24').jobs)).toEqual([6])
    expect(jobsOn(byDay, '2026-11-30')).toEqual({ jobs: [], timed: 0 })
  })

  it('puts jobs with no day under null, and leads with no time when anchored on a moment', () => {
    const cards = [job(1, { due_date: null }), job(2, { due_time: '09:00:00' })]
    expect(ids(jobsOn(jobsByDay(cards, 'due_date'), null).jobs)).toEqual([1])
    const byCreated = jobsByDay(cards, 'created_at', 'UTC')
    expect(jobsOn(byCreated, '2026-11-02').timed).toBe(0)
  })
})

describe('late', () => {
  it('is an open job whose due day has ended or whose due time has passed', () => {
    expect(isLate(job(1, { due_date: '2026-11-24T00:00:00' }), CLOCK)).toBe(true)
    expect(isLate(job(2), CLOCK)).toBe(false)
    expect(isLate(job(3, { due_time: '09:30:00' }), CLOCK)).toBe(true)
    expect(isLate(job(4, { due_time: '10:30:00' }), CLOCK)).toBe(false)
    expect(isLate(job(5, { due_date: null }), CLOCK)).toBe(false)
    const done = job(6, { due_date: '2026-11-20T00:00:00' })
    done.workflow_state = { id: 9, name: 'Done', category: 'done', color: 'green' }
    expect(isLate(done, CLOCK)).toBe(false)
  })

  it('goes by the deadline even when the job is planned for a later day', () => {
    const card = job(1, { due_date: '2026-11-24T00:00:00', start_date: '2026-11-27T00:00:00' })
    expect(planDay(card)).toBe('2026-11-27')
    expect(isLate(card, CLOCK)).toBe(true)
  })

  it('finds the first late job in reading order, inside the days asked for', () => {
    const cards = [
      job(1, { due_date: '2026-11-20T00:00:00' }),
      job(2, { due_date: '2026-10-30T00:00:00' }),
      job(3, { due_date: '2026-11-18T00:00:00', due_time: '09:00:00' }),
      job(4, { due_date: '2026-11-18T00:00:00' }, 'urgent'),
      job(5, { due_date: '2026-11-10T00:00:00' }),
    ]
    // #5 closed: not late, so not the target.
    cards[4].workflow_state = { id: 9, name: 'Done', category: 'done', color: 'green' }
    const byDay = jobsByDay(cards, 'due_date')
    expect(earliestLate(byDay, CLOCK)).toEqual({ day: '2026-10-30', card: cards[1] })
    expect(earliestLate(byDay, CLOCK, (day) => day.startsWith('2026-11'))).toEqual({
      day: '2026-11-18',
      card: cards[2],
    })
    expect(earliestLate(byDay, CLOCK, (day) => day.startsWith('2026-12'))).toBeNull()
  })
})

describe('the agenda', () => {
  const jobDays = [null, '2026-11-20', '2026-11-27', '2026-11-25', '2026-11-20']

  it('lists today, every day with a job and every day with another date, once each, in order', () => {
    expect(agendaDays(jobDays, ['2026-12-03'], TODAY)).toEqual([
      '2026-11-20',
      '2026-11-25',
      '2026-11-27',
      '2026-12-03',
    ])
    expect(agendaDays([], [], TODAY)).toEqual([TODAY])
  })

  it('opens on today with a window of days either side, counted in listed days', () => {
    const days = ['2026-11-01', '2026-11-05', '2026-11-10', TODAY, '2026-11-26', '2026-11-30', '2026-12-04']
    expect(agendaWindow(days, TODAY, 2, 2)).toEqual({
      shown: ['2026-11-05', '2026-11-10', TODAY, '2026-11-26'],
      moreEarlier: true,
      moreAhead: true,
    })
    expect(agendaWindow(days, TODAY, 7, 7)).toEqual({ shown: days, moreEarlier: false, moreAhead: false })
    expect(agendaWindow(days, TODAY, 0, 1).shown).toEqual([TODAY])
  })

  it('grows the window in steps until a day is in it', () => {
    const days = ['2026-10-01', '2026-10-05', '2026-11-01', '2026-11-10', TODAY, '2026-12-01', '2027-01-01']
    const start = { before: 1, after: 1 }
    expect(windowToReveal(days, TODAY, '2026-11-10', start, 7)).toEqual(start)
    expect(windowToReveal(days, TODAY, '2026-10-01', start, 2)).toEqual({ before: 5, after: 1 })
    expect(windowToReveal(days, TODAY, '2027-01-01', start, 7)).toEqual({ before: 1, after: 8 })
    expect(agendaWindow(days, TODAY, 5, 1).shown[0]).toBe('2026-10-01')
    // A day the agenda doesn't list leaves the window alone.
    expect(windowToReveal(days, TODAY, '2026-10-02', start, 7)).toEqual(start)
  })

  it('wants other dates from the month of its earliest job to its last, or six months ahead', () => {
    expect(agendaRange([null, '2026-09-14', TODAY], TODAY)).toEqual({ start: '2026-09-01', end: '2027-05-31' })
    expect(agendaRange(['2027-08-02'], TODAY)).toEqual({ start: '2026-11-01', end: '2027-08-31' })
  })
})

describe('carrying a job to another day', () => {
  it('moves the due date of a job planned by its deadline, leaving its time', () => {
    const card = job(1, { due_time: '09:00:00' })
    expect(movePatch(card, '2026-11-27')).toEqual({ due_date: '2026-11-27T00:00:00' })
  })

  it('moves the start date of a job planned apart from its deadline, leaving the deadline', () => {
    const card = job(1, { start_date: '2026-11-24T00:00:00', start_time: '08:00:00', due_date: '2026-11-28T00:00:00' })
    expect(movePatch(card, '2026-11-26')).toEqual({ start_date: '2026-11-26T00:00:00' })
  })

  it('writes nothing for a drop on the day the job is already on', () => {
    expect(movePatch(job(1), TODAY)).toBeNull()
    expect(movePatch(job(2, { start_date: '2026-11-24T00:00:00' }), '2026-11-24')).toBeNull()
  })

  it('takes the job out of the old day order and keeps its time, as the server stores it', () => {
    const card = job(1, { due_time: '09:00:00', rank: 'V' })
    const patch = movePatch(card, '2026-11-27')!
    const written = { ...card, ...patch, ...planPatchEffects(card, patch) }
    expect(planDay(written)).toBe('2026-11-27')
    expect(written.due_time).toBe('09:00:00')
    expect(written.rank).toBeNull()
  })
})

describe('the wall clock', () => {
  it('reads the day and time in the planning timezone, not the device', () => {
    const now = new Date('2026-11-24T22:30:00Z')
    expect(wallClock('Australia/Sydney', now)).toEqual({ day: '2026-11-25', time: '09:30' })
    expect(wallClock('America/New_York', now)).toEqual({ day: '2026-11-24', time: '17:30' })
  })
})
