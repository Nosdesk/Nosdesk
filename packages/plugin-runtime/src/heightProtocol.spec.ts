import { describe, it, expect } from 'vitest'
import {
  HAS_CONTENT_UNMEASURED,
  containerSize,
  decideHeightReport,
  type HeightInput,
} from './heightProtocol'

/** Convenience: start from "nothing reported yet". */
const at = (over: Partial<HeightInput> = {}): HeightInput => ({
  isEmpty: false,
  measuredPx: 0,
  last: null,
  ...over,
})

describe('decideHeightReport', () => {
  it('reports a measured height and carries it forward', () => {
    expect(decideHeightReport(at({ measuredPx: 210 }))).toEqual({
      report: 210,
      last: 210,
      chase: false,
    })
  })

  it('stays quiet when the height has not changed', () => {
    expect(decideHeightReport(at({ measuredPx: 210, last: 210 }))).toEqual({
      report: null,
      last: 210,
      chase: false,
    })
  })

  it('reports 0 when the plugin drew nothing, so the host can collapse it', () => {
    expect(decideHeightReport(at({ isEmpty: true }))).toEqual({
      report: 0,
      last: 0,
      chase: false,
    })
  })

  it('announces empty only once', () => {
    expect(decideHeightReport(at({ isEmpty: true, last: 0 }))).toEqual({
      report: null,
      last: 0,
      chase: false,
    })
  })

  it('emptiness wins over a stale measurement', () => {
    // The host may still be reporting the old box while the root has emptied.
    expect(decideHeightReport(at({ isEmpty: true, measuredPx: 210, last: 210 }))).toEqual({
      report: 0,
      last: 0,
      chase: false,
    })
  })

  describe('has content but cannot be measured (host has hidden us)', () => {
    it('asks for layout back and starts chasing', () => {
      expect(decideHeightReport(at({ measuredPx: 0, last: 0 }))).toEqual({
        report: HAS_CONTENT_UNMEASURED,
        last: HAS_CONTENT_UNMEASURED,
        chase: true,
      })
    })

    /**
     * Regression: `last` used to be seeded with -1, which IS the sentinel, so
     * this first measurement hit the "already announced" guard and reported
     * nothing. A panel mounted inside an already-hidden container then never
     * announced itself and sat at the iframe's default height forever. Seeding
     * with null is what makes this case reachable.
     */
    it('announces on the very first measurement, with nothing reported yet', () => {
      expect(decideHeightReport(at({ measuredPx: 0, last: null }))).toEqual({
        report: HAS_CONTENT_UNMEASURED,
        last: HAS_CONTENT_UNMEASURED,
        chase: true,
      })
    })

    it('does not re-announce while still unmeasurable', () => {
      expect(
        decideHeightReport(at({ measuredPx: 0, last: HAS_CONTENT_UNMEASURED })),
      ).toEqual({ report: null, last: HAS_CONTENT_UNMEASURED, chase: false })
    })

    it('reports the real height once layout comes back', () => {
      expect(
        decideHeightReport(at({ measuredPx: 63, last: HAS_CONTENT_UNMEASURED })),
      ).toEqual({ report: 63, last: 63, chase: false })
    })
  })

  it('never reports a negative height as a size', () => {
    // The sentinel is a signal, not a measurement: any run of the machine must
    // only ever emit 0, -1, or a positive number.
    const inputs: HeightInput[] = [
      at({ isEmpty: true }),
      at({ measuredPx: 0, last: 0 }),
      at({ measuredPx: 120, last: HAS_CONTENT_UNMEASURED }),
      at({ measuredPx: 0, last: null }),
    ]
    for (const input of inputs) {
      const { report } = decideHeightReport(input)
      if (report === null) continue
      expect(report === 0 || report === HAS_CONTENT_UNMEASURED || report > 0).toBe(true)
    }
  })

  it('a plugin that empties then refills round-trips back to a real height', () => {
    // The full recovery sequence, driven as the observers would.
    let last: number | null = null
    const seen: number[] = []
    const step = (isEmpty: boolean, measuredPx: number): void => {
      const d = decideHeightReport({ isEmpty, measuredPx, last })
      last = d.last
      if (d.report !== null) seen.push(d.report)
    }
    step(false, 210) // rendered
    step(true, 0) // emptied -> host collapses
    step(false, 0) // refilled, but hidden so unmeasurable
    step(false, 63) // host restored layout, chase measures for real
    expect(seen).toEqual([210, 0, HAS_CONTENT_UNMEASURED, 63])
  })
})

describe('containerSize', () => {
  it('buckets a sidebar panel as narrow', () => {
    expect(containerSize(334)).toBe('narrow')
  })

  it('buckets by the panel scale, not the app breakpoints', () => {
    expect(containerSize(0)).toBe('narrow')
    expect(containerSize(479)).toBe('narrow')
    expect(containerSize(480)).toBe('medium')
    expect(containerSize(570)).toBe('medium')
    expect(containerSize(767)).toBe('medium')
    expect(containerSize(768)).toBe('wide')
    expect(containerSize(1200)).toBe('wide')
  })
})
