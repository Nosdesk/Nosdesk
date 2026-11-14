import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { ref } from 'vue'
import { mountWithProviders } from '@/test/mountWithProviders'

vi.mock('@/utils/wavEncoder', () => ({ ensureWavEncoder: vi.fn().mockResolvedValue(undefined) }))
vi.mock('@/composables/useSpeechRecognition', () => ({
  useSpeechRecognition: () => ({
    isSupported: ref(false),
    fullTranscript: ref(''),
    start: vi.fn(),
    stop: vi.fn(),
    reset: vi.fn(),
  }),
}))
vi.mock('extendable-media-recorder', () => ({
  MediaRecorder: class {
    state = 'inactive'
    mimeType = 'audio/wav'
    ondataavailable: ((e: { data: Blob }) => void) | null = null
    onstop: (() => void) | null = null
    start() {
      this.state = 'recording'
    }
    stop() {
      this.state = 'inactive'
      this.ondataavailable?.({ data: new Blob([new Uint8Array(200)]) })
      this.onstop?.()
    }
  },
}))

import VoiceRecorder from '../VoiceRecorder.vue'

const session = { type: 'auto' }
let typeAtCapture: string | null = null
let wrapper: VueWrapper | null = null

beforeEach(() => {
  session.type = 'auto'
  typeAtCapture = null
  Object.defineProperty(navigator, 'audioSession', { value: session, configurable: true })
  Object.defineProperty(navigator, 'mediaDevices', {
    value: {
      getUserMedia: vi.fn(async () => {
        typeAtCapture = session.type
        return { getTracks: () => [], getAudioTracks: () => [] }
      }),
    },
    configurable: true,
  })
  vi.stubGlobal(
    'AudioContext',
    class {
      state = 'running'
      createAnalyser() {
        return { connect: vi.fn(), fftSize: 0, frequencyBinCount: 0 }
      }
      createMediaStreamSource() {
        return { connect: vi.fn() }
      }
      close() {
        return Promise.resolve()
      }
    },
  )
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.unstubAllGlobals()
  Object.defineProperty(navigator, 'audioSession', { value: undefined, configurable: true })
})

const buttonByLabel = (label: string) => wrapper!.findAll('button').find((b) => b.text().includes(label))!

// iOS can hand a page that never declared a recording session a microphone
// that delivers only silence; the recorder claims one before capture.
describe('VoiceRecorder audio session', () => {
  it('records in a play-and-record session and restores it on stop', async () => {
    wrapper = mountWithProviders(VoiceRecorder)
    await flushPromises()
    expect(typeAtCapture).toBe('play-and-record')

    await buttonByLabel('ticket-media-voice-stop').trigger('click')
    expect(session.type).toBe('auto')
    expect(wrapper.findComponent(VoiceRecorder).emitted('recordingComplete')).toHaveLength(1)
  })

  it('restores the session on cancel', async () => {
    wrapper = mountWithProviders(VoiceRecorder)
    await flushPromises()
    await buttonByLabel('ticket-media-voice-cancel').trigger('click')
    expect(session.type).toBe('auto')
  })

  it('restores the session when the recorder goes away mid-recording', async () => {
    wrapper = mountWithProviders(VoiceRecorder)
    await flushPromises()
    expect(session.type).toBe('play-and-record')
    wrapper.unmount()
    wrapper = null
    expect(session.type).toBe('auto')
  })

  it('restores the session when the microphone is refused', async () => {
    vi.mocked(navigator.mediaDevices.getUserMedia).mockRejectedValueOnce(new Error('NotAllowedError'))
    vi.spyOn(console, 'error').mockImplementation(() => {})
    wrapper = mountWithProviders(VoiceRecorder)
    await flushPromises()
    expect(session.type).toBe('auto')
    expect(wrapper.findComponent(VoiceRecorder).emitted('cancel')).toHaveLength(1)
  })
})
