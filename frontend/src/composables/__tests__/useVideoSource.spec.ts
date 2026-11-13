import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'
import { configureAssetUrl, proxiedAssetResolver } from '@nosdesk/core/transport'

const ASSET = 'nosdesk-asset://localhost'
const PATH = '/api/files/tickets/7/clip.mp4'
const PROXIED = `${ASSET}${PATH}`
const DIRECT = `https://help.example.com${PATH}`
// `MediaError` codes; jsdom has no `MediaError`.
const MEDIA_ERR_NETWORK = 2
const MEDIA_ERR_DECODE = 3

const files = vi.hoisted(() => ({
  direct: null as { url: string; path: string } | null,
  minted: 0,
  getFileToken: vi.fn(),
  discardFileToken: vi.fn(),
}))
const platform = vi.hoisted(() => ({ tauri: false }))
vi.mock('@/platform', () => ({ isTauriRuntime: () => platform.tauri }))
vi.mock('@/services/fileToken', () => ({
  directVideoUrl: () => files.direct,
  getFileToken: files.getFileToken,
  discardFileToken: files.discardFileToken,
  withFileToken: (url: string, token: string) => `${url}?token=${token}`,
}))

import { primeFirstFrame, useVideoSource, withFirstFrame } from '@/composables/useVideoSource'

function source(src: () => string | null) {
  return effectScope().run(() => useVideoSource(src))!
}

/** The app: server files resolve through the asset proxy. */
function inApp() {
  configureAssetUrl(proxiedAssetResolver(ASSET), (u) => u.replace(ASSET, ''))
}

beforeEach(() => {
  platform.tauri = false
  files.direct = null
  files.minted = 0
  files.getFileToken.mockReset()
  files.getFileToken.mockImplementation(async () => `t${++files.minted}`)
  files.discardFileToken.mockClear()
})

afterEach(() => {
  configureAssetUrl((p) => p, (u) => u)
})

describe('useVideoSource', () => {
  it("is the caller's src on the web, where the cookie authorizes it", () => {
    const video = source(() => PATH)
    expect(video.url.value).toBe(PATH)
    expect(video.direct.value).toBe(false)
    expect(video.viaProxy.value).toBe(false)
    expect(files.getFileToken).not.toHaveBeenCalled()
  })

  it('loads through the proxy in the app when there is no direct URL', () => {
    inApp()
    const video = source(() => PROXIED)
    expect(video.url.value).toBe(PROXIED)
    expect(video.viaProxy.value).toBe(true)
  })

  it("doesn't count a local file as proxied in the app", () => {
    inApp()
    const video = source(() => 'blob:tauri://localhost/0b6c')
    expect(video.viaProxy.value).toBe(false)
  })

  it('streams straight from the server with a token in the app', async () => {
    inApp()
    files.direct = { url: DIRECT, path: PATH }
    const video = source(() => PROXIED)
    // Nothing to load until the token is in.
    expect(video.url.value).toBeUndefined()
    await vi.waitFor(() => expect(video.url.value).toBe(`${DIRECT}?token=t1`))
    expect(video.direct.value).toBe(true)
    expect(video.viaProxy.value).toBe(false)
    expect(files.getFileToken).toHaveBeenCalledWith(PATH)
  })

  it('goes back to the proxy when the token cannot be minted', async () => {
    inApp()
    files.direct = { url: DIRECT, path: PATH }
    files.getFileToken.mockRejectedValue(new Error('refused'))
    const video = source(() => PROXIED)
    await vi.waitFor(() => expect(video.url.value).toBe(PROXIED))
    expect(video.direct.value).toBe(false)
    expect(video.viaProxy.value).toBe(true)
  })

  it('waits for a src, and mints nothing without one', async () => {
    files.direct = { url: DIRECT, path: PATH }
    const wanted = ref(false)
    const video = source(() => (wanted.value ? PROXIED : null))
    await nextTick()
    expect(video.url.value).toBeUndefined()
    expect(files.getFileToken).not.toHaveBeenCalled()

    wanted.value = true
    await vi.waitFor(() => expect(video.url.value).toBe(`${DIRECT}?token=t1`))
  })

  it('re-mints after a network failure, a few times, then falls back', async () => {
    inApp()
    files.direct = { url: DIRECT, path: PATH }
    const video = source(() => PROXIED)
    await vi.waitFor(() => expect(video.url.value).toBe(`${DIRECT}?token=t1`))

    for (const minted of [2, 3, 4]) {
      expect(video.recover(MEDIA_ERR_NETWORK)).toBe(true)
      await vi.waitFor(() => expect(video.url.value).toBe(`${DIRECT}?token=t${minted}`))
    }
    expect(files.discardFileToken).toHaveBeenCalledTimes(3)

    expect(video.recover(MEDIA_ERR_NETWORK)).toBe(true)
    expect(video.url.value).toBe(PROXIED)
    // Nothing direct left to recover.
    expect(video.recover(MEDIA_ERR_NETWORK)).toBe(false)
  })

  it('falls back at once for a failure that is not the network', async () => {
    files.direct = { url: DIRECT, path: PATH }
    const video = source(() => PROXIED)
    await vi.waitFor(() => expect(video.direct.value).toBe(true))
    expect(video.recover(MEDIA_ERR_DECODE)).toBe(true)
    expect(video.url.value).toBe(PROXIED)
    expect(files.minted).toBe(1)
  })

  it('falls back when a re-mint returns the token that already failed', async () => {
    files.direct = { url: DIRECT, path: PATH }
    files.getFileToken.mockImplementation(async () => 'same')
    const video = source(() => PROXIED)
    await vi.waitFor(() => expect(video.url.value).toBe(`${DIRECT}?token=same`))
    video.recover(MEDIA_ERR_NETWORK)
    await vi.waitFor(() => expect(video.url.value).toBe(PROXIED))
    expect(video.direct.value).toBe(false)
  })
})

describe('withFirstFrame', () => {
  it('starts the URL a moment in, after any query', () => {
    expect(withFirstFrame(`${DIRECT}?token=t1`)).toBe(`${DIRECT}?token=t1#t=0.001`)
    expect(withFirstFrame('blob:http://localhost/0b6c')).toBe('blob:http://localhost/0b6c#t=0.001')
  })

  it('leaves a URL that already has a fragment alone', () => {
    expect(withFirstFrame(`${PATH}#t=5`)).toBe(`${PATH}#t=5`)
  })
})

describe('primeFirstFrame', () => {
  const mutedAtPlay: boolean[] = []
  beforeEach(() => {
    mutedAtPlay.length = 0
  })

  /** A paused element whose `play` resolves, firing `playing`, unless refused. */
  function element(refused = false) {
    const video = document.createElement('video')
    let paused = true
    Object.defineProperty(video, 'paused', { get: () => paused })
    video.play = vi.fn(async () => {
      if (refused) throw new Error('NotAllowedError')
      mutedAtPlay.push(video.muted)
      paused = false
      video.dispatchEvent(new Event('playing'))
    })
    video.pause = vi.fn(() => {
      paused = true
    })
    return video
  }

  it('does nothing outside the app', () => {
    const video = element()
    const settled = vi.fn()
    primeFirstFrame(video, settled)
    expect(video.play).not.toHaveBeenCalled()
    expect(settled).toHaveBeenCalledOnce()
  })

  it('plays muted to the first frame, pauses and restores the muted state', async () => {
    platform.tauri = true
    const video = element()
    const settled = vi.fn()
    primeFirstFrame(video, settled)
    await nextTick()
    expect(mutedAtPlay).toEqual([true])
    expect(video.pause).toHaveBeenCalledOnce()
    expect(video.muted).toBe(false)
    expect(settled).toHaveBeenCalledOnce()
  })

  it('leaves a real play running when cancelled first', () => {
    platform.tauri = true
    const video = element()
    video.play = vi.fn(() => new Promise<void>(() => {}))
    const settled = vi.fn()
    const cancel = primeFirstFrame(video, settled)
    cancel()
    video.dispatchEvent(new Event('playing'))
    expect(video.pause).not.toHaveBeenCalled()
    expect(video.muted).toBe(false)
    expect(settled).toHaveBeenCalledOnce()
  })

  it('settles when the web view refuses the play', async () => {
    platform.tauri = true
    const video = element(true)
    const settled = vi.fn()
    primeFirstFrame(video, settled)
    await nextTick()
    expect(video.pause).not.toHaveBeenCalled()
    expect(video.muted).toBe(false)
    expect(settled).toHaveBeenCalledOnce()
  })
})
