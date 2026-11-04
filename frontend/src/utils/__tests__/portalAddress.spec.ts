import { describe, expect, it } from 'vitest'
import { portalAddressFrom } from '@/utils/portalAddress'

describe('portalAddressFrom', () => {
  it('takes a web address with or without its scheme and path', () => {
    expect(portalAddressFrom('support.acme.com', '')).toBe('https://support.acme.com/')
    expect(portalAddressFrom(' https://Support.Acme.com/tickets?x=1 ', '')).toBe('https://support.acme.com/')
    expect(portalAddressFrom('http://acme.nosdesk.app', 'nosdesk.app')).toBe('https://acme.nosdesk.app/')
  })

  it('takes a short name on Nosdesk Cloud', () => {
    expect(portalAddressFrom('acme', 'nosdesk.app')).toBe('https://acme.nosdesk.app/')
    expect(portalAddressFrom('acme', '')).toBeNull()
  })

  it('refuses what cannot be an address', () => {
    for (const input of ['', 'acme corp', 'a..b.com', '-acme.com', 'acme.com:8080', 'user@acme.com', 'javascript:alert(1)']) {
      expect(portalAddressFrom(input, 'nosdesk.app'), input).toBeNull()
    }
  })
})
