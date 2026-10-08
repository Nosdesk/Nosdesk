import { describe, expect, it } from 'vitest'
import { withSavedRow } from '@/utils/defaultRows'

const rows = [
  { id: 1, is_default: true },
  { id: 2, is_default: false },
]

describe('withSavedRow', () => {
  it('moves the default to a row saved as the default', () => {
    expect(withSavedRow(rows, { id: 2, is_default: true })).toEqual([
      { id: 1, is_default: false },
      { id: 2, is_default: true },
    ])
  })

  it('adds a new default row and clears the old one', () => {
    expect(withSavedRow(rows, { id: 3, is_default: true })).toEqual([
      { id: 1, is_default: false },
      { id: 2, is_default: false },
      { id: 3, is_default: true },
    ])
  })

  it('leaves the default alone otherwise', () => {
    expect(withSavedRow(rows, { id: 2, is_default: false })).toEqual(rows)
  })
})
