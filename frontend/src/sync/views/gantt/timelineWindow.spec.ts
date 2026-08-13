import { describe, it, expect } from 'vitest'
import { computeTimelineWindow, landingScrollTop } from './timelineWindow'

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

/**
 * Regression: the mobile gantt opened at the top of a canvas whose start is
 * dragged back by old cycle bands, so the live work sat several screens down.
 * Measured before the fix: 6 bars, 0 visible on open, first bar 2988px down.
 */
describe('landingScrollTop', () => {
  const viewportHeight = 650
  const canvasHeight = 3420

  it('opens on today when the work is around now', () => {
    const todayY = 2000
    const top = landingScrollTop({
      todayY,
      firstBarTop: 1950,
      lastBarBottom: 2400,
      viewportHeight,
      canvasHeight,
    })
    // On screen, and below the top edge rather than pinned to it, so a little
    // history stays visible for context.
    expect(todayY).toBeGreaterThan(top)
    expect(todayY).toBeLessThan(top + viewportHeight)
  })

  it('puts the first bar on screen in the case that regressed', () => {
    const top = landingScrollTop({
      todayY: 3000,
      firstBarTop: 2988,
      lastBarBottom: 3300,
      viewportHeight,
      canvasHeight,
    })
    expect(2988).toBeGreaterThanOrEqual(top)
    expect(2988).toBeLessThanOrEqual(top + viewportHeight)
  })

  it('opens on the last bar when every plan is in the past', () => {
    const top = landingScrollTop({
      todayY: 9999,
      firstBarTop: 400,
      lastBarBottom: 900,
      viewportHeight,
      canvasHeight,
    })
    expect(900).toBeGreaterThanOrEqual(top)
    expect(900).toBeLessThanOrEqual(top + viewportHeight)
  })

  it('opens on the first bar when every plan is still ahead', () => {
    const top = landingScrollTop({
      todayY: -500,
      firstBarTop: 1800,
      lastBarBottom: 2200,
      viewportHeight,
      canvasHeight,
    })
    expect(1800).toBeGreaterThanOrEqual(top)
    expect(1800).toBeLessThanOrEqual(top + viewportHeight)
  })

  it('falls back to today when nothing is scheduled', () => {
    const todayY = 720
    const top = landingScrollTop({
      todayY,
      firstBarTop: null,
      lastBarBottom: null,
      viewportHeight,
      canvasHeight,
    })
    expect(todayY).toBeGreaterThan(top)
    expect(todayY).toBeLessThan(top + viewportHeight)
  })

  it('never scrolls past either end of the canvas', () => {
    expect(
      landingScrollTop({ todayY: 0, firstBarTop: null, lastBarBottom: null, viewportHeight, canvasHeight }),
    ).toBe(0)
    expect(
      landingScrollTop({ todayY: 99999, firstBarTop: null, lastBarBottom: null, viewportHeight, canvasHeight }),
    ).toBe(canvasHeight - viewportHeight)
  })

  it('does not scroll a canvas that fits the viewport', () => {
    expect(
      landingScrollTop({ todayY: 200, firstBarTop: 100, lastBarBottom: 300, viewportHeight: 650, canvasHeight: 500 }),
    ).toBe(0)
  })
})
