import { describe, expect, it } from 'vitest'

import { withJsonError } from '@nosdesk/core/services/importService'

describe('import downloads', () => {
  it('read a failed download\'s JSON body back so its code can be read', async () => {
    const blob = new Blob([JSON.stringify({ error: 'gone', code: 'import_file_missing' })], {
      type: 'application/json',
    })
    const failure = { response: { status: 409, data: blob as unknown } }
    await expect(withJsonError(() => Promise.reject(failure))).rejects.toBe(failure)
    expect(failure.response.data).toEqual({ error: 'gone', code: 'import_file_missing' })
  })

  it('leave a body that is not JSON as it is', async () => {
    const blob = new Blob(['<html>'])
    const failure = { response: { status: 502, data: blob as unknown } }
    await expect(withJsonError(() => Promise.reject(failure))).rejects.toBe(failure)
    expect(failure.response.data).toBe(blob)
  })

  it('pass a download through', async () => {
    await expect(withJsonError(() => Promise.resolve('file'))).resolves.toBe('file')
  })
})
