import { describe, expect, it, vi } from 'vitest'
import { retryAfterMs, withRateLimitRetry } from './rateLimitRetry'

const status = (code: number, data?: unknown, headers?: Record<string, string>) =>
  Object.assign(new Error(`HTTP ${code}`), { response: { status: code, data, headers } })

describe('withRateLimitRetry', () => {
  it('waits out a 429 and returns what the retry gets', async () => {
    const request = vi
      .fn()
      .mockRejectedValueOnce(status(429, { code: 'RATE_LIMITED', retry_after: 3 }))
      .mockResolvedValueOnce({ uuid: 'user-1' })
    const wait = vi.fn(async (_ms: number) => {})

    await expect(withRateLimitRetry(request, { wait })).resolves.toEqual({ uuid: 'user-1' })
    expect(request).toHaveBeenCalledTimes(2)
    expect(wait).toHaveBeenCalledWith(3000)
  })

  it('gives a 401 after a 429 back to the caller, unretried', async () => {
    const unauthorized = status(401)
    const request = vi
      .fn()
      .mockRejectedValueOnce(status(429, undefined, { 'retry-after': '2' }))
      .mockRejectedValueOnce(unauthorized)
    const wait = vi.fn(async (_ms: number) => {})

    await expect(withRateLimitRetry(request, { wait })).rejects.toBe(unauthorized)
    expect(request).toHaveBeenCalledTimes(2)
    expect(wait).toHaveBeenCalledWith(2000)
  })

  it('gives the 429 back once the total wait is used up', async () => {
    const limited = status(429, { retry_after: 40 })
    const request = vi.fn().mockRejectedValue(limited)
    const wait = vi.fn(async (_ms: number) => {})

    await expect(withRateLimitRetry(request, { wait, maxWaitMs: 60_000 })).rejects.toBe(limited)
    expect(wait.mock.calls.map(([ms]) => ms)).toEqual([40_000, 20_000])
    expect(request).toHaveBeenCalledTimes(3)
  })

  it('never retries other errors', async () => {
    const failure = status(500)
    const request = vi.fn().mockRejectedValue(failure)
    const wait = vi.fn(async (_ms: number) => {})

    await expect(withRateLimitRetry(request, { wait })).rejects.toBe(failure)
    expect(request).toHaveBeenCalledTimes(1)
    expect(wait).not.toHaveBeenCalled()
  })
})

describe('retryAfterMs', () => {
  it('prefers the body, then the header, then one second', () => {
    expect(retryAfterMs(status(429, { retry_after: 5 }, { 'retry-after': '9' }))).toBe(5000)
    expect(retryAfterMs(status(429, {}, { 'retry-after': '9' }))).toBe(9000)
    expect(retryAfterMs(status(429))).toBe(1000)
  })
})
