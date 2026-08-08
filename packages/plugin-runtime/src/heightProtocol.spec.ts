import { describe, it, expect } from 'vitest'
import { containerSize, decideHeightReport, type HeightInput } from './heightProtocol'

const at = (over: Partial<HeightInput> = {}): HeightInput => ({
  isEmpty: false,
  measuredPx: 0,
  last: null,
  ...over,
})

describe('decideHeightReport', () => {
  it('reports a measured height and carries it forward', () => {
    expect(decideHeightReport(at({ measuredPx: 210 }))).toEqual({ report: 210, last: 210 })
  })

  it('stays quiet when the height has not changed', () => {
    expect(decideHeightReport(at({ measuredPx: 210, last: 210 }))).toEqual({
      report: null,
      last: 210,
    })
  })

  it('reports 0 when the plugin drew nothing, so the host can collapse it', () => {
    expect(decideHeightReport(at({ isEmpty: true }))).toEqual({ report: 0, last: 0 })
  })

  it('announces empty only once', () => {
    expect(decideHeightReport(at({ isEmpty: true, last: 0 }))).toEqual({ report: null, last: 0 })
  })

  it('emptiness wins over a stale measurement', () => {
    expect(decideHeightReport(at({ isEmpty: true, measuredPx: 210, last: 210 }))).toEqual({
      report: 0,
      last: 0,
    })
  })

  it('never reports a negative height', () => {
    // Collapsed ancestors can produce odd measurements; a report is a size.
    expect(decideHeightReport(at({ measuredPx: -5 }))).toEqual({ report: 0, last: 0 })
  })

  /**
   * The case the old three-state protocol existed to handle. Collapsing with
   * `block-size: 0` instead of `display: none` keeps layout alive in the guest,
   * so a plugin that fills in after a fetch is just another content change and
   * needs no sentinel or chase loop.
   */
  it('a plugin that empties then refills round-trips with no extra machinery', () => {
    let last: number | null = null
    const seen: number[] = []
    const step = (isEmpty: boolean, measuredPx: number): void => {
      const d = decideHeightReport({ isEmpty, measuredPx, last })
      last = d.last
      if (d.report !== null) seen.push(d.report)
    }
    step(true, 0) // mounted, drew nothing -> host collapses
    step(false, 63) // filled in later; still measurable, so simply reported
    expect(seen).toEqual([0, 63])
  })

  it('only ever emits zero or a positive height', () => {
    const runs: HeightInput[] = [
      at({ isEmpty: true }),
      at({ measuredPx: 0, last: 0 }),
      at({ measuredPx: 120, last: 0 }),
      at({ measuredPx: -1 }),
      at({ measuredPx: 0, last: null }),
    ]
    for (const input of runs) {
      const { report } = decideHeightReport(input)
      if (report === null) continue
      expect(report).toBeGreaterThanOrEqual(0)
    }
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
