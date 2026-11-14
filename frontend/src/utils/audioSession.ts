/**
 * WebKit's audio session (`navigator.audioSession`, Safari and WKWebView on
 * iOS 17 and later). Left on `auto`, iOS can keep the session set up for
 * playback, after a video has played or even on a fresh launch, and a
 * microphone opened then delivers silence: the track is live and unmuted but
 * every sample is zero. Declaring `play-and-record` before the microphone
 * opens gives the recording real input. Engines without the API need nothing.
 */
type AudioSessionType = 'auto' | 'playback' | 'transient' | 'transient-solo' | 'ambient' | 'play-and-record'

interface AudioSession {
  type: AudioSessionType
}

/**
 * Switch the page's audio session to recording and return the function that
 * puts back the type it had, so audio played afterwards (a video, a voice
 * note) is routed for playback again. Restoring twice is harmless.
 */
export const claimRecordingSession = (): (() => void) => {
  const session = (navigator as Navigator & { audioSession?: AudioSession }).audioSession
  if (!session) return () => {}
  const previous = session.type
  session.type = 'play-and-record'
  let restored = false
  return () => {
    if (restored) return
    restored = true
    session.type = previous
  }
}
