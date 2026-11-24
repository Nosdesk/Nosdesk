import { describe, expect, it } from 'vitest'
import {
  bookPatch,
  formatWallTime,
  loadHours,
  loadMinutes,
  planField,
  planPatchEffects,
  planTime,
  planTimePatch,
  SIZE_MINUTES,
  sizeOf,
  toHhmm,
  toMinutes,
} from '@nosdesk/core/utils/ticketPlan'

describe('the plan time', () => {
  it('comes from the start date when there is one, else the due date', () => {
    const due = { due_date: '2026-11-27T00:00:00', due_time: '14:30:00' }
    expect(planField(due)).toBe('due')
    expect(planTime(due)).toBe('14:30:00')

    const both = { ...due, start_date: '2026-11-24T00:00:00', start_time: null }
    expect(planField(both)).toBe('start')
    expect(planTime(both)).toBeNull()
    expect(planTimePatch(both, '09:00:00')).toEqual({ start_time: '09:00:00' })
    expect(planTimePatch(due, null)).toEqual({ due_time: null })
  })

  it('books a job on its own day with the time alone', () => {
    const due = { due_date: '2026-11-24T00:00:00' }
    expect(bookPatch(due, '2026-11-24', '09:00:00')).toEqual({ due_time: '09:00:00' })
    const started = { start_date: '2026-11-24T00:00:00', due_date: '2026-11-27T00:00:00' }
    expect(bookPatch(started, '2026-11-24', '09:00:00')).toEqual({ start_time: '09:00:00' })
  })

  it('books a job carried in from an earlier day by moving the date that supplies its day', () => {
    const carried = {
      start_date: '2026-11-20T00:00:00',
      start_time: '08:00:00',
      due_date: '2026-11-27T00:00:00',
      rank: 'V',
    }
    const patch = bookPatch(carried, '2026-11-24', '10:30:00')
    expect(patch).toEqual({ start_date: '2026-11-24T00:00:00', start_time: '10:30:00' })
    // The server clears the rank on the move; the stores mirror that.
    expect(planPatchEffects(carried, patch)).toEqual({ rank: null })
  })

  it('converts between a wall-clock time and minutes, clamped to the day', () => {
    expect(toMinutes('09:30:00')).toBe(570)
    expect(toMinutes('14:05')).toBe(845)
    expect(toHhmm(570)).toBe('09:30')
    expect(toHhmm(-30)).toBe('00:00')
    expect(toHhmm(24 * 60)).toBe('23:45')
  })

  it('formats a time in the locale with no zone shift', () => {
    expect(formatWallTime('14:30:00', 'en-US')).toMatch(/^2:30\sPM$/)
    expect(formatWallTime('21:15:00', 'en-GB')).toBe('21:15')
  })
})

describe('the size', () => {
  it('reads minutes as a letter by range, so any value shows one', () => {
    expect(sizeOf(null)).toBeNull()
    expect(sizeOf(undefined)).toBeNull()
    expect(sizeOf(1)).toBe('s')
    expect(sizeOf(60)).toBe('s')
    expect(sizeOf(61)).toBe('m')
    expect(sizeOf(240)).toBe('m')
    expect(sizeOf(241)).toBe('l')
  })

  it('writes presets that read back as their own letter', () => {
    expect(SIZE_MINUTES).toEqual({ s: 30, m: 120, l: 360 })
    for (const [size, minutes] of Object.entries(SIZE_MINUTES)) expect(sizeOf(minutes)).toBe(size)
  })

  it('sums the sized jobs and rounds the load to the half hour', () => {
    expect(loadMinutes([{ estimate_minutes: 30 }, { estimate_minutes: null }, { estimate_minutes: 120 }])).toBe(150)
    expect(loadHours(150)).toBe(2.5)
    expect(loadHours(500)).toBe(8.5)
    expect(loadHours(0)).toBe(0)
  })
})

describe('what a plan write brings with it', () => {
  const placed = {
    due_date: '2026-11-21T00:00:00',
    due_time: '14:30:00',
    rank: 'V',
    assignee_uuid: 'sam',
  }

  it('clears a time with its date', () => {
    expect(planPatchEffects(placed, { due_date: null })).toEqual({ due_time: null, rank: null })
    const started = { start_date: '2026-11-21T00:00:00', start_time: '09:00:00' }
    expect(planPatchEffects(started, { start_date: null })).toEqual({ start_time: null })
  })

  it('leaves a time the write sets itself', () => {
    expect(planPatchEffects(placed, { due_date: null, due_time: null })).toEqual({ rank: null })
  })

  it('clears the rank when the job leaves its day or its person', () => {
    expect(planPatchEffects(placed, { due_date: '2026-11-22T00:00:00' })).toEqual({ rank: null })
    expect(planPatchEffects(placed, { assignee_uuid: 'priya' })).toEqual({ rank: null })
  })

  it('keeps the rank on the same day, on a time change and when the write places it', () => {
    expect(planPatchEffects(placed, { due_date: '2026-11-21T00:00:00' })).toEqual({})
    expect(planPatchEffects(placed, { due_time: null })).toEqual({})
    expect(planPatchEffects(placed, { assignee_uuid: 'sam' })).toEqual({})
    expect(planPatchEffects(placed, { due_date: '2026-11-22T00:00:00', rank: 'a' })).toEqual({})
  })

  it('reads the plan day, so a deadline edit under a start date keeps the place', () => {
    const booked = { ...placed, start_date: '2026-11-20T00:00:00' }
    expect(planPatchEffects(booked, { due_date: '2026-11-28T00:00:00' })).toEqual({})
    expect(planPatchEffects(booked, { start_date: null })).toEqual({ rank: null })
  })
})
