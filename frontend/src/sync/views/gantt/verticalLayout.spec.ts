import { describe, it, expect } from 'vitest'
import { assignLanes, canvasWidth, fidelityFor, laneCount, laneWidth } from './verticalLayout'

const day = (n: number): Date => new Date(2026, 7, 10 + n)
/** `n` tickets all in flight at once. */
const overlapping = (n: number) =>
  Array.from({ length: n }, (_, i) => ({ start: day(i % 2), end: day(6 + (i % 3)) }))

const PHONE = 390

describe('assignLanes', () => {
  it('puts sequential work in one column', () => {
    const placed = assignLanes([
      { start: day(0), end: day(2) },
      { start: day(2), end: day(4) },
      { start: day(4), end: day(6) },
    ])
    expect(laneCount(placed)).toBe(1)
  })

  it('uses one column per simultaneous ticket', () => {
    expect(laneCount(assignLanes(overlapping(4)))).toBe(4)
    expect(laneCount(assignLanes(overlapping(8)))).toBe(8)
  })

  it('reuses a column as soon as it frees up', () => {
    // Two overlap, the third starts after the first ends.
    const placed = assignLanes([
      { start: day(0), end: day(3) },
      { start: day(1), end: day(5) },
      { start: day(3), end: day(6) },
    ])
    expect(laneCount(placed)).toBe(2)
  })

  it('is stable regardless of input order', () => {
    const items = overlapping(6)
    const a = laneCount(assignLanes(items))
    const b = laneCount(assignLanes([...items].reverse()))
    expect(a).toBe(b)
  })
})

/**
 * The design's load-bearing claim: horizontal gantt is bounded by time span
 * (90 days will never fit 390px), vertical by concurrency. This pins where
 * concurrency actually bites on a phone, so the trade is a measured one.
 */
describe('concurrency on a 390px phone', () => {
  const counts = [1, 2, 3, 4, 5, 6, 8, 10, 12, 15]
  const tiers = counts.map((n) => {
    const w = laneWidth(PHONE, n)
    return {
      n,
      width: Math.round(w),
      fidelity: fidelityFor(w, 200),
      pans: canvasWidth(PHONE, n) > PHONE,
    }
  })
  const at = (n: number) => tiers.find((t) => t.n === n)!

  it('keeps full cards to 2 parallel tickets', () => {
    expect(tiers.filter((t) => t.fidelity === 'full').map((t) => t.n)).toEqual([1, 2])
  })

  /**
   * The load-bearing property. Dividing the viewport by concurrency alone put 5
   * parallel tickets at 67px, one under what a titled chip needs, and dropped
   * the WHOLE view to id-only marks; 5 in flight is an ordinary week. The floor
   * means a block stays readable at any concurrency, so the view never has a
   * count at which it stops working.
   */
  it('stays readable at every concurrency, never falling to marks', () => {
    expect(tiers.filter((t) => t.fidelity === 'mark')).toEqual([])
    for (const t of tiers) expect(t.width).toBeGreaterThanOrEqual(80)
  })

  it('shares the viewport to 4, then holds the floor and pans', () => {
    expect(at(4)).toMatchObject({ width: 84, pans: false })
    expect(at(5)).toMatchObject({ width: 80, pans: true })
    expect(tiers.filter((t) => t.pans).map((t) => t.n)).toEqual([5, 6, 8, 10, 12, 15])
  })

  it('pans by an amount bounded by concurrency, not by time span', () => {
    // The horizontal gantt this replaced needed 3737px of canvas for a 90-day
    // window. Here 8 parallel tickets cost 306px of pan, and the span is free.
    expect(Math.round(canvasWidth(PHONE, 8) - PHONE)).toBe(306)
    expect(canvasWidth(PHONE, 4)).toBe(PHONE)
  })
})

describe('fidelityFor', () => {
  it('demotes a short block however wide its column', () => {
    // A one-day deadline marker is ~36px tall; a title clips mid-word in it.
    expect(fidelityFor(300, 36)).toBe('mark')
    expect(fidelityFor(300, 200)).toBe('full')
  })

  it('demotes a narrow column however tall the block', () => {
    expect(fidelityFor(40, 400)).toBe('mark')
  })

  it('is monotonic in width at a readable height', () => {
    const order = { mark: 0, compact: 1, full: 2 } as const
    let last = -1
    for (const w of [20, 68, 100, 132, 300]) {
      const rank = order[fidelityFor(w, 200)]
      expect(rank).toBeGreaterThanOrEqual(last)
      last = rank
    }
  })
})
