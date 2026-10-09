import axios, { AxiosError, type AxiosAdapter } from 'axios'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// The portal client recovers a lapsed session one way: a 401 refreshes once
// and the request is sent again. Any 403 is a refusal, not a lapsed session.

const appRouter = vi.hoisted(() => ({
  currentRoute: { value: { name: 'tickets', fullPath: '/tickets' } },
  push: vi.fn(),
}))
vi.mock('../router', () => ({ default: appRouter }))
vi.mock('../embed', () => ({
  isEmbed: false,
  embedBearer: () => null,
  embedHome: '/embed',
  signInEmbedded: vi.fn(),
}))

import portalApi from '../api'

type Reply = { status: number; data?: unknown }

/** Answers each request with the next reply; returns how many were sent. */
function serve(...replies: Reply[]): () => number {
  let sent = 0
  const adapter: AxiosAdapter = async (config) => {
    sent++
    const reply = replies.shift() ?? { status: 200 }
    const response = { status: reply.status, statusText: '', data: reply.data ?? {}, headers: {}, config }
    if (reply.status >= 400) throw new AxiosError('refused', 'ERR_BAD_REQUEST', config, null, response)
    return response
  }
  portalApi.defaults.adapter = adapter
  return () => sent
}

const refresh = vi.spyOn(axios, 'post')

beforeEach(() => {
  refresh.mockReset()
  refresh.mockResolvedValue({ status: 200, data: {} })
  appRouter.push.mockClear()
})

describe('portal API client', () => {
  it('refreshes on a 401 and sends the request again', async () => {
    const sent = serve({ status: 401 }, { status: 200, data: { ok: true } })

    const res = await portalApi.post('/tickets', {})

    expect(res.data).toEqual({ ok: true })
    expect(refresh).toHaveBeenCalledTimes(1)
    expect(refresh.mock.calls[0][0]).toBe('/api/portal/auth/refresh')
    expect(sent()).toBe(2)
  })

  it('retries once only', async () => {
    const sent = serve({ status: 401 }, { status: 401 })

    await expect(portalApi.post('/tickets', {})).rejects.toBeTruthy()

    expect(refresh).toHaveBeenCalledTimes(1)
    expect(sent()).toBe(2)
  })

  it('treats a CSRF refusal as a refusal, not a lapsed session', async () => {
    const sent = serve({ status: 403, data: { error: 'CSRF token required', code: 'csrf_missing' } })

    await expect(portalApi.post('/tickets', {})).rejects.toBeTruthy()

    expect(refresh).not.toHaveBeenCalled()
    expect(sent()).toBe(1)
  })
})

describe('portal API client when the refresh fails', () => {
  it('stays put and keeps the session when the refresh is unavailable', async () => {
    serve({ status: 401 })
    refresh.mockRejectedValue(
      new AxiosError('unavailable', 'ERR_BAD_RESPONSE', undefined, null, {
        status: 503,
        statusText: '',
        data: {},
        headers: {},
        config: {} as never,
      }),
    )

    const failed = await portalApi.post('/tickets', {}).catch((e: AxiosError) => e)

    expect(failed.response?.status).toBe(401)
    expect(appRouter.push).not.toHaveBeenCalled()
  })

  it('stays put when offline', async () => {
    serve({ status: 401 })
    refresh.mockRejectedValue(new AxiosError('Network Error', 'ERR_NETWORK'))

    await expect(portalApi.post('/tickets', {})).rejects.toBeTruthy()

    expect(appRouter.push).not.toHaveBeenCalled()
  })

  it('goes to sign-in when the refresh is refused', async () => {
    serve({ status: 401 })
    refresh.mockRejectedValue(
      new AxiosError('refused', 'ERR_BAD_REQUEST', undefined, null, {
        status: 401,
        statusText: '',
        data: {},
        headers: {},
        config: {} as never,
      }),
    )

    await expect(portalApi.post('/tickets', {})).rejects.toBeTruthy()

    expect(appRouter.push).toHaveBeenCalledTimes(1)
    expect(appRouter.push.mock.calls[0][0]).toMatchObject({ path: '/login' })
  })
})
