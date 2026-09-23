import { describe, expect, it } from 'vitest'

import {
  ApiError,
  AuthenticationError,
  PermissionError,
  ValidationError,
  createErrorFromResponse,
  errorBody,
  errorCode,
  errorStatus,
} from './errors'

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

describe('typed errors keep the response', () => {
  // Call sites written against raw axios errors read err.response; the
  // interceptor hands them typed errors instead, so the typed error must
  // carry the same shape or those checks silently never match.
  it('a 409 keeps its status and code where err.response reads them', () => {
    const err = createErrorFromResponse(failure(409, { error: 'Last owner', code: 'last_owner' }))
    expect(err.response?.status).toBe(409)
    expect(err.response?.data?.code).toBe('last_owner')
    expect(errorCode(err)).toBe('last_owner')
    expect(errorStatus(err)).toBe(409)
  })

  it('403 and 422 keep theirs too', () => {
    const denied = createErrorFromResponse(failure(403, { error: 'No', code: 'FORBIDDEN' }))
    expect(denied).toBeInstanceOf(PermissionError)
    expect(errorStatus(denied)).toBe(403)
    expect(errorCode(denied)).toBe('FORBIDDEN')

    const invalid = createErrorFromResponse(failure(422, { error: 'Bad', code: 'license_expired' }))
    expect(invalid).toBeInstanceOf(ValidationError)
    expect(errorCode(invalid)).toBe('license_expired')
  })

  it('reads a raw axios error the same way', () => {
    const raw = failure(409, { error: 'Conflict', code: 'schema_invalidates_existing_assets', invalid_count: 2 })
    expect(errorCode(raw)).toBe('schema_invalidates_existing_assets')
    expect(errorBody(raw)?.invalid_count).toBe(2)
    expect(errorStatus(raw)).toBe(409)
  })

  it('has nothing to say about an error with no response', () => {
    expect(errorCode(new Error('boom'))).toBeUndefined()
    expect(errorStatus(undefined)).toBeUndefined()
    expect(errorBody(failure(500, 'plain text body'))).toBeUndefined()
  })
})
