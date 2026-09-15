import { describe, expect, it } from 'vitest'

import { ApiError, AuthenticationError, PermissionError, createErrorFromResponse } from './errors'

const failure = (status: number, data: unknown) => ({
  response: { status, data, config: { url: '/api/x' } },
})

describe('createErrorFromResponse', () => {
  it('reads the canonical { error, code } envelope', () => {
    const err = createErrorFromResponse(failure(409, { error: 'Slug already taken', code: 'CONFLICT' }))
    expect(err).toBeInstanceOf(ApiError)
    expect(err.message).toBe('Slug already taken')
  })

  it('still reads the older { status, message } login bodies', () => {
    const err = createErrorFromResponse(failure(500, { status: 'error', message: 'Error generating token' }))
    expect(err.message).toBe('Error generating token')
  })

  it('maps gate denials to the typed auth errors with their text', () => {
    const denied = createErrorFromResponse(failure(403, { error: 'You are not a member of this workspace', code: 'FORBIDDEN' }))
    expect(denied).toBeInstanceOf(PermissionError)
    expect(denied.message).toBe('You are not a member of this workspace')

    const anon = createErrorFromResponse(failure(401, { error: 'Authentication required', code: 'AUTH_REQUIRED' }))
    expect(anon).toBeInstanceOf(AuthenticationError)
    expect(anon.message).toBe('Authentication required')
  })

  it('falls back to a generic message when the body carries neither field', () => {
    expect(createErrorFromResponse(failure(502, {})).message).toBe('An error occurred')
  })
})
