import { describe, expect, it } from 'vitest'

import { initialsFrom, monogramColor, monogramDataUri, monogramHue } from './monogram'

describe('initialsFrom', () => {
  it('takes up to two initials', () => {
    expect(initialsFrom('Venus')).toBe('V')
    expect(initialsFrom('Acme Support')).toBe('AS')
    expect(initialsFrom('One Two Three')).toBe('OT')
  })

  it('handles separators and empty input', () => {
    expect(initialsFrom('north-west')).toBe('NW')
    expect(initialsFrom('  spaced   out ')).toBe('SO')
    expect(initialsFrom('')).toBe('?')
    expect(initialsFrom('   ')).toBe('?')
  })
})

describe('monogramHue', () => {
  it('is stable for the same key', () => {
    expect(monogramHue('abc')).toBe(monogramHue('abc'))
  })

  it('is in range', () => {
    for (const key of ['', 'a', 'workspace', '3f2b1c9d-0000-4000-8000-000000000001']) {
      const hue = monogramHue(key)
      expect(hue).toBeGreaterThanOrEqual(0)
      expect(hue).toBeLessThan(360)
    }
  })

  // The reason this is keyed on the uuid rather than the name: uuids share
  // long prefixes, so a weak hash would put every workspace on one colour.
  it('separates keys that share a prefix', () => {
    const a = '3f2b1c9d-0000-4000-8000-000000000001'
    const b = '3f2b1c9d-0000-4000-8000-000000000002'
    expect(monogramHue(a)).not.toBe(monogramHue(b))
  })
})

describe('monogramDataUri', () => {
  it('renders the initials into an svg data uri', () => {
    const uri = monogramDataUri('Venus', 'key-1')
    expect(uri.startsWith('data:image/svg+xml,')).toBe(true)
    expect(decodeURIComponent(uri)).toContain('>V<')
  })

  it('escapes names that would otherwise break the document', () => {
    const decoded = decodeURIComponent(monogramDataUri('<script>', 'key-1'))
    expect(decoded).not.toContain('<script>')
    expect(decoded).toContain('&lt;')
  })

  it('uses the key, not the name, for colour', () => {
    // Same first letter, different workspace: the marks must not match.
    expect(monogramDataUri('Support', 'key-a')).not.toBe(monogramDataUri('Support', 'key-b'))
    expect(monogramColor('key-a')).not.toBe(monogramColor('key-b'))
  })
})
