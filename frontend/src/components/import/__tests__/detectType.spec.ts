import { describe, expect, it } from 'vitest'

import { detectOtherType, listHeaders, normalizeHeader } from '../detectType'

describe('normalizeHeader', () => {
  it('reads headers the way the server matches them', () => {
    expect(normalizeHeader(' Serial No. ')).toBe('serialno')
    expect(normalizeHeader('Asset #')).toBe('assetno')
    expect(normalizeHeader('S/N')).toBe('sn')
    expect(normalizeHeader('E-mail')).toBe('email')
  })
})

describe('detectOtherType', () => {
  it('reads Email with First and Last and no tag as users', () => {
    expect(detectOtherType('assets', ['First', 'Last', 'Email', 'Dept'])).toEqual({
      type: 'users',
      headers: ['First', 'Last', 'Email'],
    })
    expect(detectOtherType('tickets', ['E-mail', 'Surname'])).toEqual({
      type: 'users',
      headers: ['E-mail', 'Surname'],
    })
  })

  it('leaves an asset file with an assignee email alone', () => {
    expect(
      detectOtherType('assets', ['Asset #', 'Device Name', 'Assigned To', 'Email', 'First Name']),
    ).toBeNull()
  })

  it('reads a tag or serial with a name as assets', () => {
    expect(detectOtherType('users', ['Name', 'Email', 'Asset #', 'S/N'])).toEqual({
      type: 'assets',
      headers: ['Name', 'Asset #', 'S/N'],
    })
    expect(detectOtherType('tickets', ['Device Name', 'Serial Number'])?.type).toBe('assets')
  })

  it('never suggests the type already chosen', () => {
    expect(detectOtherType('users', ['First', 'Last', 'Email'])).toBeNull()
    expect(detectOtherType('assets', ['Name', 'Asset Tag'])).toBeNull()
  })

  it('needs both halves of a hint', () => {
    expect(detectOtherType('assets', ['Email', 'Phone'])).toBeNull()
    expect(detectOtherType('users', ['Asset #', 'Location'])).toBeNull()
  })
})

describe('listHeaders', () => {
  it('joins headers as a sentence list', () => {
    expect(listHeaders(['Email', 'First', 'Last'], 'en-US')).toBe('Email, First, and Last')
    expect(listHeaders(['Email'], 'en-US')).toBe('Email')
  })
})
