import { describe, expect, it } from 'vitest'
import { peopleColumns, peopleFacets, peopleGroupAxes } from '@/utils/peopleListLayout'

// A requester-role member gets only other people's names and avatars, so the
// People list shows just that; staff keep the working roster.

describe('the People list for a requester-role member', () => {
  it('shows names and avatars only', () => {
    expect(peopleColumns(false, false)).toEqual(['user'])
    expect(peopleColumns(false, true)).toEqual(['user'])
  })

  it('offers no role filter or role and join grouping', () => {
    expect(peopleFacets(false)).toEqual(['name'])
    expect(peopleGroupAxes(false)).toEqual(['status'])
  })
})

describe('the People list for staff', () => {
  it('keeps the working columns', () => {
    expect(peopleColumns(true, false)).toEqual([
      'user',
      'email',
      'role',
      'open_ticket_count',
      'device_count',
      'created_at',
    ])
    expect(peopleColumns(true, true)).toEqual(['user', 'email', 'created_at'])
    expect(peopleFacets(true)).toEqual(['name', 'role'])
    expect(peopleGroupAxes(true)).toEqual(['role', 'status', 'joined'])
  })
})
