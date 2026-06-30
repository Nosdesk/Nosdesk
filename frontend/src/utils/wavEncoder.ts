import { register } from 'extendable-media-recorder'
import { connect } from 'extendable-media-recorder-wav-encoder'

/**
 * Register the WAV AudioWorklet encoder for `extendable-media-recorder`, once
 * per page (`register()` throws if called twice).
 *
 * Voice notes record to WAV on every surface because iOS WKWebView's native
 * `MediaRecorder` only emits fragmented mp4 (zero-duration moov), which won't
 * play from a `blob:` URL and can't be decoded for a waveform. WAV is a
 * complete, self-describing file that plays and decodes on every engine, so it
 * gives one recording path for web + mobile. See `VoiceRecorder.vue`.
 */
let registration: Promise<unknown> | null = null

export const ensureWavEncoder = (): Promise<unknown> => {
  if (!registration) {
    registration = connect().then((port) => register(port))
  }
  return registration
}
