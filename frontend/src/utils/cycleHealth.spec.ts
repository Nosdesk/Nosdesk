import { describe, expect, it } from 'vitest'
import { cycleHealth } from './cycleHealth'

const day = (iso: string) => new Date(`${iso}T12:00:00Z`).getTime()

describe('cycleHealth', () => {
  it('reads an active cycle with no tickets as empty, not "not started"', () => {
    expect(
      cycleHealth({ total: 0, completed: 0, startAt: '2026-09-01', endAt: '2026-09-30', now: day('2026-09-10') }),
    ).toBe('empty')
  })

  it('reads a cycle before its start date as not started, tickets or not', () => {
    const future = { startAt: '2026-10-01', endAt: '2026-10-14', now: day('2026-09-24') }
    expect(cycleHealth({ total: 0, completed: 0, ...future })).toBe('not-started')
    // Used to read "on track" (negative elapsed fraction).
    expect(cycleHealth({ total: 5, completed: 0, ...future })).toBe('not-started')
  })

  it('still forecasts a started cycle with work', () => {
    expect(
      cycleHealth({ total: 10, completed: 0, startAt: '2026-09-01', endAt: '2026-09-30', now: day('2026-09-24') }),
    ).toBe('behind')
  })
})
