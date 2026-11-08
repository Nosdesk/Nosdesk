import { beforeEach, describe, expect, it, vi } from 'vitest'

const client = vi.hoisted(() => ({ get: vi.fn() }))
vi.mock('@nosdesk/core/apiClient', () => ({ default: client }))

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

describe('template download', () => {
  beforeEach(() => client.get.mockReset())

  // Through the API client, which adds the workspace selection header; a
  // plain link to the URL went without it and hosted workspaces refused it.
  it('fetches through the API client, adding only what it was asked for', async () => {
    client.get.mockResolvedValue({ data: new Blob(['a']), headers: {} })
    await importService.template('users')
    expect(client.get).toHaveBeenLastCalledWith('/admin/import/template/users', {
      params: {},
      responseType: 'blob',
    })
    await importService.template('assets', 'camera', 'csv')
    expect(client.get).toHaveBeenLastCalledWith('/admin/import/template/assets', {
      params: { kind: 'camera' },
      responseType: 'blob',
    })
    await importService.template('assets', 'camera', 'xlsx')
    expect(client.get).toHaveBeenLastCalledWith('/admin/import/template/assets', {
      params: { kind: 'camera', format: 'xlsx' },
      responseType: 'blob',
    })
  })

  it("saves under the server's name, else one of its own", async () => {
    const blob = new Blob(['a'])
    client.get.mockResolvedValueOnce({
      data: blob,
      headers: { 'content-disposition': 'attachment; filename="nosdesk-assets-camera-template.xlsx"' },
    })
    await expect(importService.template('assets', 'camera', 'xlsx')).resolves.toEqual({
      blob,
      filename: 'nosdesk-assets-camera-template.xlsx',
    })
    client.get.mockResolvedValueOnce({ data: blob, headers: {} })
    await expect(importService.template('tickets', undefined, 'xlsx')).resolves.toEqual({
      blob,
      filename: 'nosdesk-tickets-template.xlsx',
    })
  })

  it("reads a refusal's JSON body back", async () => {
    const failure = {
      response: {
        status: 400,
        data: new Blob([JSON.stringify({ error: 'No workspace selected', code: 'NO_WORKSPACE_SELECTED' })]) as unknown,
      },
    }
    client.get.mockRejectedValueOnce(failure)
    await expect(importService.template('assets')).rejects.toBe(failure)
    expect(failure.response.data).toEqual({ error: 'No workspace selected', code: 'NO_WORKSPACE_SELECTED' })
  })
})
