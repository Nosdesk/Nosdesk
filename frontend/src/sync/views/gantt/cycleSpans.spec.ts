import { describe, it, expect } from 'vitest'
import { differenceInCalendarDays } from 'date-fns'
import {
  cycleBodyClass,
  cycleStripClass,
  datedCycleSpans,
  projectCycleBand,
} from './cycleSpans'
import type { GanttCycle } from './types'

const cycle = (partial: Partial<GanttCycle> & Pick<GanttCycle, 'uuid' | 'name'>): GanttCycle => ({
  id: 1,
  state: 'active',
  ...partial,
})

describe('datedCycleSpans', () => {
  it('skips cycles missing either date', () => {
    expect(
      datedCycleSpans([
        cycle({ uuid: 'a', name: 'A', start_at: '2026-08-01' }),
        cycle({ uuid: 'b', name: 'B', end_at: '2026-08-14' }),
        cycle({ uuid: 'c', name: 'C' }),
      ]),
    ).toEqual([])
  })

  it('treats end_at as inclusive (half-open end is the next day)', () => {
    const [span] = datedCycleSpans([
      cycle({
        uuid: 's1',
        name: 'Sprint 1',
        start_at: '2026-08-10T00:00:00',
        end_at: '2026-08-14T00:00:00',
        state: 'active',
      }),
    ])
    expect(span.label).toBe('Sprint 1')
    expect(span.state).toBe('active')
    // 10,11,12,13,14 = 5 inclusive days → exclusive end is the 15th.
    expect(differenceInCalendarDays(span.endExclusive, span.start)).toBe(5)
    expect(span.start.getDate()).toBe(10)
    expect(span.endExclusive.getDate()).toBe(15)
  })

  it('drops inverted or zero-length ranges', () => {
    expect(
      datedCycleSpans([
        cycle({
          uuid: 'bad',
          name: 'Bad',
          start_at: '2026-08-14',
          end_at: '2026-08-10',
        }),
      ]),
    ).toEqual([])
  })
})

describe('projectCycleBand', () => {
  const dayOffset = (from: Date, to: Date) => differenceInCalendarDays(to, from)
  const canvasStart = new Date(2026, 7, 1) // 1 Aug local
  const px = 36

  it('projects a fully in-window span', () => {
    const [span] = datedCycleSpans([
      cycle({
        uuid: 's',
        name: 'S',
        start_at: '2026-08-03',
        end_at: '2026-08-05',
      }),
    ])
    // 3 days inclusive → 3 * 36 = 108px, offset 2 days = 72.
    const band = projectCycleBand(span, canvasStart, 30 * px, px, dayOffset)
    expect(band).toMatchObject({ offset: 72, extent: 108, label: 'S' })
  })

  it('clips to the canvas and skips fully outside spans', () => {
    const [inside] = datedCycleSpans([
      cycle({
        uuid: 'in',
        name: 'In',
        start_at: '2026-07-28',
        end_at: '2026-08-02',
      }),
    ])
    // Starts before canvas (Jul 28); inclusive end Aug 2 → exclusive Aug 3.
    // On a canvas from Aug 1 that is two days: Aug 1 and Aug 2 → 2 * 36.
    const clipped = projectCycleBand(inside, canvasStart, 30 * px, px, dayOffset)
    expect(clipped?.offset).toBe(0)
    expect(clipped?.extent).toBe(2 * px)

    const [outside] = datedCycleSpans([
      cycle({
        uuid: 'out',
        name: 'Out',
        start_at: '2026-09-01',
        end_at: '2026-09-10',
      }),
    ])
    expect(projectCycleBand(outside, canvasStart, 14 * px, px, dayOffset)).toBeNull()
  })
})

describe('cycle style helpers', () => {
  it('marks active stronger than planned or completed', () => {
    expect(cycleStripClass('active')).toContain('accent')
    expect(cycleBodyClass('active')).toContain('accent')
    expect(cycleStripClass('planned')).not.toContain('accent')
    expect(cycleBodyClass('completed')).not.toContain('accent')
  })
})
