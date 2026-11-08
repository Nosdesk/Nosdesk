import { describe, expect, it } from 'vitest'

import { importService, withJsonError } from '@nosdesk/core/services/importService'

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

describe('template URL', () => {
  it('is unchanged for CSV and adds the format for Excel', () => {
    expect(importService.templateUrl('users')).toBe('/api/admin/import/template/users')
    expect(importService.templateUrl('assets', 'camera')).toBe(
      '/api/admin/import/template/assets?kind=camera',
    )
    expect(importService.templateUrl('assets', 'camera', 'csv')).toBe(
      '/api/admin/import/template/assets?kind=camera',
    )
    expect(importService.templateUrl('assets', 'camera', 'xlsx')).toBe(
      '/api/admin/import/template/assets?kind=camera&format=xlsx',
    )
    expect(importService.templateUrl('tickets', undefined, 'xlsx')).toBe(
      '/api/admin/import/template/tickets?format=xlsx',
    )
  })
})
