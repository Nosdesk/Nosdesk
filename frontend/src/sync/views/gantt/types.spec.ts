import { describe, expect, it } from 'vitest'
import { setDateConfig } from '@nosdesk/core/utils/dateUtils'
import { dayLabel, dayRangeLabel, naiveDay } from './types'

// The board's dates are local midnights. Whatever zone the user has
// configured, a label must name the day the bar sits on.
describe('gantt day labels', () => {
  const local = new Date(2026, 8, 17)

  it('name the local day regardless of the configured zone', () => {
    setDateConfig({ defaultLocale: 'en-US', defaultTimezone: 'Etc/GMT+12' })
    expect(dayLabel(local, 'day')).toBe('17')
    expect(dayLabel(local)).toBe('Sep 17')
    expect(dayRangeLabel(local, new Date(2026, 8, 20)).replace(/\s+/g, ' ')).toBe('Sep 17 – 20')
  })

  it('write back the local day as a naive timestamp', () => {
    expect(naiveDay(local)).toBe('2026-09-17T00:00:00')
  })
})
