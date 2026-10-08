import { describe, expect, it } from 'vitest'
import { isInAppPath, MAX_RETURN_PATH_LENGTH } from '@/utils/inAppPath'
import casesJson from '../../../../backend/tests/fixtures/in_app_paths.json?raw'

// The same cases the server's `safe_post_login_location` tests read, so both
// sides apply one rule.
const cases = JSON.parse(casesJson) as { in_app: string[]; not_in_app: string[] }
const longest = `/${'a'.repeat(MAX_RETURN_PATH_LENGTH - 1)}`
const IN_APP = [...cases.in_app, longest]
const NOT_IN_APP = [...cases.not_in_app, `${longest}a`]

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
