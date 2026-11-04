import { afterEach, describe, expect, it, vi } from 'vitest'
import { AxiosError, CanceledError } from 'axios'
import { isCancellation } from '@nosdesk/core/utils/cancellation'
import { logger } from '@nosdesk/core/utils/logger'

// A request the app chose not to finish (a sign-out, a workspace switch, a
// newer search) isn't a failure. Signing out on Account > Security logged six
// red "Failed to ..." lines, one per request the sign-out cancelled.

describe('isCancellation', () => {
  it('recognises every way a request is cancelled', () => {
    expect(isCancellation(new CanceledError('The session ended before this request was sent'))).toBe(true)
    expect(isCancellation(new DOMException('aborted', 'AbortError'))).toBe(true)
    expect(isCancellation(new Error('REQUEST_CANCELLED'))).toBe(true)
    expect(isCancellation(Object.assign(new Error('canceled'), { code: 'ERR_CANCELED' }))).toBe(true)
  })

  it('leaves failures alone', () => {
    expect(isCancellation(new AxiosError('Request failed with status code 500', 'ERR_BAD_RESPONSE'))).toBe(false)
    expect(isCancellation(new Error('Network Error'))).toBe(false)
    expect(isCancellation('REQUEST_CANCELLED')).toBe(false)
    expect(isCancellation(null)).toBe(false)
    expect(isCancellation(undefined)).toBe(false)
  })
})

describe('logging a cancelled request', () => {
  afterEach(() => vi.restoreAllMocks())

  function spyConsole() {
    return {
      error: vi.spyOn(console, 'error').mockImplementation(() => {}),
      warn: vi.spyOn(console, 'warn').mockImplementation(() => {}),
      debug: vi.spyOn(console, 'debug').mockImplementation(() => {}),
    }
  }

  it('is a debug line, not an error or a warning', () => {
    const out = spyConsole()
    logger.error('Failed to get MFA status', { error: new CanceledError('The session ended before this request was sent') })
    logger.warn('Failed to load passkeys', { err: new DOMException('aborted', 'AbortError') })
    expect(out.error).not.toHaveBeenCalled()
    expect(out.warn).not.toHaveBeenCalled()
    expect(out.debug).toHaveBeenCalledTimes(2)
  })

  it('still logs a real failure as an error', () => {
    const out = spyConsole()
    logger.error('Failed to get MFA status', { error: new AxiosError('Request failed with status code 500') })
    expect(out.error).toHaveBeenCalledTimes(1)
  })
})
