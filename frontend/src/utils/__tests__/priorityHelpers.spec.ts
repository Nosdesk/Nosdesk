import { describe, expect, it } from 'vitest'
import {
  comparePriority,
  PRIORITY_OPTIONS,
  priorityRank,
} from '@nosdesk/core/constants/ticketOptions'
import { inlinePriorityClass, priorityToneClass } from '@/utils/priorityHelpers'

describe('priorities', () => {
  it('compare by severity', () => {
    const shuffled = ['low', 'urgent', 'none', 'high', 'medium']
    expect([...shuffled].sort(comparePriority)).toEqual(['none', 'low', 'medium', 'high', 'urgent'])
    // Most severe first, as the options list them.
    expect([...shuffled].sort((a, b) => comparePriority(b, a))).toEqual(
      PRIORITY_OPTIONS.map((o) => o.value),
    )
  })

  it('rank anything that is not a priority as none', () => {
    expect(priorityRank('critical')).toBe(priorityRank('none'))
    expect(priorityRank(null)).toBe(0)
  })

  it('give urgent its own colours', () => {
    expect(inlinePriorityClass('urgent')).toBe('text-rose-500')
    expect(priorityToneClass('urgent')).toContain('rose')
    expect(inlinePriorityClass('none')).toBeNull()
  })
})
