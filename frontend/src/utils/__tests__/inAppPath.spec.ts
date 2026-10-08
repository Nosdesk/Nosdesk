import { describe, expect, it } from 'vitest'
import { isInAppPath } from '@/utils/inAppPath'

// The same cases as the server's
// `post_login_location_refuses_paths_a_browser_reads_as_another_host`
// (backend/src/handlers/auth_providers.rs): both sides apply one rule.
const IN_APP = ['/', '/acme/tickets/12', '/acme/tickets/12?tab=notes#c4', '/search?q=%20vpn']
const NOT_IN_APP = [
  '',
  'tickets/12',
  '//evil.example',
  '/\\evil.example',
  '/x\\y',
  '/\t/evil.example',
  '/\n/evil.example',
  '/a b',
  'https://evil.example',
  'javascript:alert(1)',
  '/\u00e9',
  '/%2F%2Fevil.example',
  '/%2fevil.example',
  '/%5Cevil.example',
  '/x%5Cy',
]

describe('isInAppPath', () => {
  it('accepts a path in the app, with its query and hash', () => {
    for (const path of IN_APP) expect(isInAppPath(path), path).toBe(true)
  })

  it('refuses anything that could leave the app, encoded or not', () => {
    for (const value of NOT_IN_APP) expect(isInAppPath(value), JSON.stringify(value)).toBe(false)
  })

  it('refuses a value that is not a string', () => {
    for (const value of [undefined, null, ['/a']]) expect(isInAppPath(value)).toBe(false)
  })
})
