import { describe, expect, it } from 'vitest'
import { mergeDestination } from '@/utils/mergeDestination'

const E = { id: 10, number: 7 }
const F = { id: 11, number: 8 }

describe('mergeDestination', () => {
  it('is the preferred ticket when it is one of them', () => {
    expect(mergeDestination([E, F], 11)).toBe(F)
    expect(mergeDestination([F, E], 10)).toBe(E)
  })

  it('is the oldest without a preferred ticket, whatever the order', () => {
    expect(mergeDestination([F, E])).toBe(E)
    expect(mergeDestination([E, F], null)).toBe(E)
  })

  it('is the oldest when the preferred ticket is not one of them', () => {
    expect(mergeDestination([F, E], 99)).toBe(E)
  })

  it('goes by created time when the tickets carry it', () => {
    const older = { id: 20, created: '2026-01-01T00:00:00Z' }
    const newer = { id: 5, created: '2026-02-01T00:00:00Z' }
    expect(mergeDestination([newer, older])).toBe(older)
  })

  it('is nothing for no tickets', () => {
    expect(mergeDestination([])).toBeNull()
  })
})
