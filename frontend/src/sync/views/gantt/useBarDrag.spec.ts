import { describe, it, expect } from 'vitest'
import { dayOffsetAt } from './useBarDrag'

describe('dayOffsetAt', () => {
  it('snaps to whole days from the axis origin', () => {
    // origin at 100px, 36px/day: client 100 → day 0, 118 → day 0, 119 → day 1
    expect(dayOffsetAt(100, 100, 36)).toBe(0)
    expect(dayOffsetAt(100 + 17, 100, 36)).toBe(0)
    expect(dayOffsetAt(100 + 18, 100, 36)).toBe(1)
    expect(dayOffsetAt(100 + 36 * 3, 100, 36)).toBe(3)
  })

  it('works equally for a vertical origin (top) as for left', () => {
    // Pure of axis: only the numbers matter. Vertical passes rect.top.
    expect(dayOffsetAt(400, 200, 36)).toBe(Math.round(200 / 36))
  })
})
