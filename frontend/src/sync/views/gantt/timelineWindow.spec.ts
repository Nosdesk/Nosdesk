import { describe, it, expect } from 'vitest'
import { computeTimelineWindow } from './timelineWindow'

const day = (n: number): Date => new Date(2026, 7, 10 + n)

describe('computeTimelineWindow', () => {
  const pxPerDay = 36
  const viewportHeight = 360 // 10 screenful days

  it('falls back to a fortnight when nothing is dated', () => {
    const w = computeTimelineWindow([], {
      viewportHeight,
      pxPerDay,
      now: day(0),
    })
    expect(w.days).toBe(14)
  })

  it('covers ticket spans with one-day padding', () => {
    const w = computeTimelineWindow(
      [{ start: day(0), end: day(4) }],
      { viewportHeight: 100, pxPerDay, now: day(0) },
    )
    // start = day(-1); end pad +2 on max → at least 4 - (-1) + 2 = 7
    expect(w.start.getDate()).toBe(9)
    expect(w.days).toBeGreaterThanOrEqual(7)
  })

  it('expands to include cycle extents beyond tickets', () => {
    const ticketsOnly = computeTimelineWindow(
      [{ start: day(0), end: day(2) }],
      { viewportHeight: 100, pxPerDay, now: day(0) },
    )
    const withCycle = computeTimelineWindow(
      [
        { start: day(0), end: day(2) },
        { start: day(-5), end: day(20) },
      ],
      { viewportHeight: 100, pxPerDay, now: day(0) },
    )
    expect(withCycle.days).toBeGreaterThan(ticketsOnly.days)
    expect(withCycle.start.getTime()).toBeLessThan(ticketsOnly.start.getTime())
  })

  it('never shorter than a screenful of time', () => {
    const w = computeTimelineWindow(
      [{ start: day(0), end: day(1) }],
      { viewportHeight, pxPerDay, now: day(0) },
    )
    expect(w.days).toBeGreaterThanOrEqual(Math.ceil(viewportHeight / pxPerDay))
  })
})
