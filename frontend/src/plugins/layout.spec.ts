import { describe, it, expect } from 'vitest'
import { breakpointFor } from './layout'
import { BREAKPOINTS } from '@/composables/useMobileDetection'

/**
 * The app breakpoint is the one layout fact a sandboxed plugin cannot work out
 * for itself: a 336px sidebar panel measures the same on a phone as on a 4K
 * display. It has to agree with the host's own scale, or a plugin branching on
 * `md` means something different from the app branching on `md`.
 */
describe('breakpointFor', () => {
  it('agrees with the host breakpoint scale at every boundary', () => {
    expect(breakpointFor(BREAKPOINTS.sm - 1)).toBe('base')
    expect(breakpointFor(BREAKPOINTS.sm)).toBe('sm')
    expect(breakpointFor(BREAKPOINTS.md - 1)).toBe('sm')
    expect(breakpointFor(BREAKPOINTS.md)).toBe('md')
    expect(breakpointFor(BREAKPOINTS.lg - 1)).toBe('md')
    expect(breakpointFor(BREAKPOINTS.lg)).toBe('lg')
    expect(breakpointFor(BREAKPOINTS.xl - 1)).toBe('lg')
    expect(breakpointFor(BREAKPOINTS.xl)).toBe('xl')
  })

  it('classifies the viewports the mobile audit drives', () => {
    expect(breakpointFor(390)).toBe('base')
    expect(breakpointFor(700)).toBe('sm')
    expect(breakpointFor(900)).toBe('md')
    expect(breakpointFor(1680)).toBe('xl')
  })

  it('is total for degenerate widths', () => {
    expect(breakpointFor(0)).toBe('base')
    expect(breakpointFor(99_999)).toBe('xl')
  })
})
