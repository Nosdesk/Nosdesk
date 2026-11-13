import { computed, readonly, ref, watch, type ComputedRef, type Ref } from 'vue'
import { logger } from '@nosdesk/core/utils/logger'
import { assetPath } from '@nosdesk/core/transport'
import {
  directVideoUrl,
  discardFileToken,
  getFileToken,
  withFileToken,
  type DirectVideo,
} from '@/services/fileToken'
import { isTauriRuntime } from '@/platform'

// Tokens re-minted for one video's file. Each lasts 15 minutes, so a few cover
// a long watch; past that, or for a failure that isn't the network, the video
// goes back through the app's proxy.
const MAX_RECOVERIES = 3
// `MediaError.MEDIA_ERR_NETWORK`, spelled out for environments without it.
const MEDIA_ERR_NETWORK = 2

export interface VideoSource {
  /** The URL a `<video>` loads, without a media fragment. Unset until it is
   *  ready, and while the caller has no `src`. */
  url: Readonly<Ref<string | undefined>>
  /** Whether `url` streams straight from the server with a file token. */
  direct: Readonly<Ref<boolean>>
  /** Whether `url` loads through the app's asset proxy, which fetches each
   *  range whole: fine for the player, too costly for a grid of tiles. */
  viaProxy: ComputedRef<boolean>
  /**
   * After a direct load failed with `code` (a `MediaError` code): re-mint its
   * token, or go back to the caller's `src`. False when the load wasn't
   * direct, so there is nothing to recover.
   */
  recover: (code: number | undefined) => boolean
}

/**
 * The URL to load a video at `src` (already through `assetUrl`) from. In the
 * app a server video streams straight from the server with a token for its
 * file, not through the app's proxy, which can't stream (see
 * services/fileToken). Everywhere else, and if the direct load fails, it is
 * `src`: on the web the session cookie authorizes it.
 */
export function useVideoSource(src: () => string | null | undefined): VideoSource {
  const url = ref<string | undefined>(undefined)
  const isDirect = ref(false)
  let target: DirectVideo | null = null
  let resolving = 0
  let recoveries = 0

  const fallBack = () => {
    resolving++
    target = null
    isDirect.value = false
    url.value = src() || undefined
  }

  const resolve = async (fresh = false) => {
    const mine = ++resolving
    const from = src()
    const next = from ? directVideoUrl(from) : null
    target = next
    if (!next) {
      isDirect.value = false
      url.value = from || undefined
      return
    }
    if (fresh) discardFileToken(next.path)
    try {
      const token = await getFileToken(next.path)
      if (mine !== resolving || target !== next) return
      const tokened = withFileToken(next.url, token)
      // A token minted in the same second is the same token; that load already
      // failed, so it wasn't the token.
      if (fresh && tokened === url.value) {
        fallBack()
      } else {
        isDirect.value = true
        url.value = tokened
      }
    } catch (error) {
      logger.debug('[useVideoSource] file token unavailable, loading through the app', { error })
      if (mine === resolving) fallBack()
    }
  }

  watch(
    src,
    () => {
      recoveries = 0
      void resolve()
    },
    { immediate: true },
  )

  // A direct load fails as a network error once its token runs out (the server
  // refuses the next range). Then re-mint and carry on; any other failure, or
  // one too many, goes back to the app's proxy.
  const recover = (code: number | undefined): boolean => {
    if (!target) return false
    if (code === MEDIA_ERR_NETWORK && recoveries < MAX_RECOVERIES) {
      recoveries++
      void resolve(true)
    } else {
      fallBack()
    }
    return true
  }

  const viaProxy = computed(() => {
    const loading = url.value
    return !!loading && !isDirect.value && assetPath(loading) !== loading
  })

  return { url: readonly(url), direct: readonly(isDirect), viaProxy, recover }
}

/**
 * `url` with a media fragment that starts it a moment in, so WebKit decodes
 * and paints the first frame under `preload="metadata"` instead of a black
 * box. The fragment never leaves the element (fetch strips it, and the app's
 * proxy reads only the path and query), so a file token's path still matches.
 */
export function withFirstFrame(url: string): string {
  return url.includes('#') ? url : `${url}#t=0.001`
}

/**
 * Load and paint the first frame of a `video` that hasn't played, in the app.
 * The app's iOS web view loads no media data before playback (Safari does, the
 * embedded web view doesn't), so `preload` and the first-frame fragment alone
 * leave a blank box until play. A muted play that pauses on its first frame
 * loads and paints it; the app's web view allows playback without a gesture.
 * Elsewhere this does nothing. `onSettled` runs once the priming play has
 * paused, failed or been cancelled. Returns a cancel for a real play that
 * starts first: it stops the pause and restores the element's muted state.
 */
export function primeFirstFrame(video: HTMLVideoElement, onSettled?: () => void): () => void {
  if (!isTauriRuntime() || !video.paused) {
    onSettled?.()
    return () => {}
  }
  const wasMuted = video.muted
  let done = false
  const finish = (pause: boolean) => {
    if (done) return
    done = true
    video.removeEventListener('playing', onPlaying)
    if (pause) video.pause()
    video.muted = wasMuted
    onSettled?.()
  }
  const onPlaying = () => finish(true)
  video.addEventListener('playing', onPlaying)
  video.muted = true
  video.play().catch(() => finish(false))
  return () => finish(false)
}
