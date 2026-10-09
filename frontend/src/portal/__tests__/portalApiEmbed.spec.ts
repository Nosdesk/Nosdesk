import type { AxiosAdapter } from 'axios'
import { describe, expect, it, vi } from 'vitest'

// Embedded, the portal sends the visitor's bearer. When the frame sits on the
// portal's own site it also has the portal cookies, and the CSRF token goes
// with the bearer so the server can check it.

vi.mock('../router', () => ({ default: { currentRoute: { value: { name: 'embed', fullPath: '/embed' } }, push: vi.fn() } }))
vi.mock('../embed', () => ({
  isEmbed: true,
  embedBearer: () => 'visitor-token',
  embedHome: '/embed',
  signInEmbedded: vi.fn(),
}))

import portalApi from '../api'

function capture(): Record<string, unknown>[] {
  const sent: Record<string, unknown>[] = []
  const adapter: AxiosAdapter = async (config) => {
    sent.push({ ...config.headers })
    return { status: 200, statusText: 'OK', data: {}, headers: {}, config }
  }
  portalApi.defaults.adapter = adapter
  return sent
}

describe('embedded portal API client', () => {
  it('sends the bearer, and the CSRF token when the frame has one', async () => {
    const sent = capture()
    document.cookie = 'portal_csrf=frame-csrf; path=/'

    await portalApi.post('/tickets', {})

    expect(sent[0]['Authorization']).toBe('Bearer visitor-token')
    expect(sent[0]['X-CSRF-Token']).toBe('frame-csrf')
  })
})
