import { afterEach, describe, expect, it } from 'vitest'
import { getDateConfig, setDateConfig } from '@nosdesk/core/utils/dateUtils'
import { clockOf, dayOf, momentOf, recordedLater } from '../assetTimeline'

// A record's time of day is the workspace's wall clock, whatever zone the
// browser is in, and reads back as it was typed.

const saved = getDateConfig()
afterEach(() => setDateConfig(saved))

describe('assetTimeline', () => {
  it('turns a day and a time into an instant in the workspace zone and back', () => {
    setDateConfig({ defaultTimezone: 'Australia/Sydney' })
    const at = momentOf('2023-05-03', '09:30')
    expect(at).toBe('2023-05-02T23:30:00.000Z')
    expect(clockOf(at)).toBe('09:30')
    expect(dayOf(at)).toBe('2023-05-03')

    setDateConfig({ defaultTimezone: 'America/New_York' })
    const evening = momentOf('2023-11-05', '23:45')
    expect(evening).toBe('2023-11-06T04:45:00.000Z')
    expect(clockOf(evening)).toBe('23:45')
    expect(dayOf(evening)).toBe('2023-11-05')
  })

  it('counts a record as written later from the second day on', () => {
    setDateConfig({ defaultTimezone: 'UTC' })
    expect(recordedLater({ day: '2026-06-09', recorded_at: '2026-06-10T23:00:00Z' })).toBe(false)
    expect(recordedLater({ day: '2026-06-09', recorded_at: '2026-06-11T00:30:00Z' })).toBe(true)
  })
})
