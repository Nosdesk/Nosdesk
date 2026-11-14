import { afterEach, describe, expect, it } from 'vitest'
import { claimRecordingSession } from '@/utils/audioSession'

const setAudioSession = (session: { type: string } | undefined) => {
  Object.defineProperty(navigator, 'audioSession', { value: session, configurable: true })
}

describe('claimRecordingSession', () => {
  afterEach(() => {
    setAudioSession(undefined)
  })

  it('switches the session to recording and restores the previous type once', () => {
    const session = { type: 'auto' }
    setAudioSession(session)

    const restore = claimRecordingSession()
    expect(session.type).toBe('play-and-record')

    restore()
    expect(session.type).toBe('auto')

    // A later type set elsewhere survives a second restore.
    session.type = 'playback'
    restore()
    expect(session.type).toBe('playback')
  })

  it('does nothing where the engine has no audio session', () => {
    setAudioSession(undefined)
    const restore = claimRecordingSession()
    expect(() => restore()).not.toThrow()
  })
})
