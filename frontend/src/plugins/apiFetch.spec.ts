/**
 * `api.fetch` hands back the upstream body verbatim: text is not JSON-quoted,
 * and a 204 or 304 (which a Response must be built without a body for) reads
 * as '' instead of throwing.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Plugin } from '@nosdesk/core/types/plugin'

const proxyRequest = vi.hoisted(() => vi.fn())
vi.mock('@nosdesk/core/services/pluginService', () => ({ default: { proxyRequest } }))
// The real store pulls in the router and every view.
vi.mock('@/stores/auth', () => ({ useAuthStore: vi.fn() }))

const { createPluginAPI } = await import('./api')

const plugin = {
  uuid: '0190a5b2-0000-7000-8000-000000000005',
  name: 'github-integration',
  consented_permissions: ['network:api.github.com'],
  manifest: { permissions: ['network:api.github.com'] },
} as unknown as Plugin

afterEach(() => proxyRequest.mockReset())

describe('api.fetch bodies', () => {
  it('keeps a text body as text', async () => {
    proxyRequest.mockResolvedValue({ status: 200, headers: { 'content-type': 'text/plain' }, body: 'plain words' })
    const res = await createPluginAPI(plugin).fetch('https://api.github.com/zen')
    await expect(res.text()).resolves.toBe('plain words')
  })

  it('keeps a JSON body as the upstream sent it', async () => {
    proxyRequest.mockResolvedValue({ status: 200, headers: {}, body: '{"number":12}' })
    const res = await createPluginAPI(plugin).fetch('https://api.github.com/repos/o/r/issues/12')
    await expect(res.json()).resolves.toEqual({ number: 12 })
  })

  it.each([204, 205, 304])('returns %i with an empty body', async (status) => {
    proxyRequest.mockResolvedValue({ status, headers: { etag: '"abc"' }, body: '' })
    const res = await createPluginAPI(plugin).fetch('https://api.github.com/repos/o/r/issues/12', {
      headers: { 'If-None-Match': '"abc"' },
    })
    expect(res.status).toBe(status)
    expect(res.headers.get('etag')).toBe('"abc"')
    await expect(res.text()).resolves.toBe('')
    expect(proxyRequest.mock.calls[0][1].headers).toEqual({ 'If-None-Match': '"abc"' })
  })
})
