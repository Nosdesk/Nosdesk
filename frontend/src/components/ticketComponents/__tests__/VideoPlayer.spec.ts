import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'
import { COARSE_POINTER_QUERY } from '@/composables/useCoarsePointer'

const PROXIED = 'nosdesk-asset://localhost/api/files/tickets/7/clip.mp4'
const PATH = '/api/files/tickets/7/clip.mp4'
const DIRECT = `https://help.example.com${PATH}`
// The media fragment a first load carries, for its first frame.
const FIRST_FRAME = '#t=0.001'
// `MediaError` codes; jsdom has no `MediaError`.
const MEDIA_ERR_NETWORK = 2
const MEDIA_ERR_DECODE = 3

const files = vi.hoisted(() => ({
  direct: null as { url: string; path: string } | null,
  minted: 0,
  getFileToken: vi.fn(),
  discardFileToken: vi.fn(),
}))
vi.mock('@/services/fileToken', () => ({
  directVideoUrl: () => files.direct,
  getFileToken: files.getFileToken,
  discardFileToken: files.discardFileToken,
  withFileToken: (url: string, token: string) => `${url}?token=${token}`,
}))

// The real priming does nothing outside the app; a test that needs one in
// flight installs its own here.
type Prime = (video: HTMLVideoElement, onSettled?: () => void) => () => void
const priming = vi.hoisted(() => ({ prime: null as Prime | null }))
vi.mock('@/composables/useVideoSource', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/composables/useVideoSource')>()
  return {
    ...actual,
    primeFirstFrame: (video: HTMLVideoElement, onSettled?: () => void) =>
      (priming.prime ?? actual.primeFirstFrame)(video, onSettled),
  }
})

import VideoPlayer from '../VideoPlayer.vue'

function player(src: string, props: { maxHeight?: string } = {}) {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  return mount(VideoPlayer, {
    props: { src, ...props },
    global: { plugins: [createFluentVue({ bundles: [bundle] })] },
  })
}

/** Stand in for the device's primary input: a finger (`coarse`) or a mouse. */
function pointer(coarse: boolean) {
  vi.stubGlobal('matchMedia', (query: string): MediaQueryList => ({
    matches: coarse && query === COARSE_POINTER_QUERY,
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  }))
}

beforeEach(() => {
  files.direct = null
  files.minted = 0
  files.getFileToken.mockReset()
  files.getFileToken.mockImplementation(async () => `t${++files.minted}`)
  files.discardFileToken.mockClear()
  priming.prime = null
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('VideoPlayer source', () => {
  it('plays the given source where there is no direct URL, from its first frame', async () => {
    const video = player('/api/files/tickets/7/clip.mp4').find('video')
    await flushPromises()
    expect(video.attributes('src')).toBe(`/api/files/tickets/7/clip.mp4${FIRST_FRAME}`)
    expect(video.attributes('preload')).toBe('metadata')
  })

  it('shows the first frame of a file staged in the composer', async () => {
    const video = player('blob:http://localhost/0b6c').find('video')
    await flushPromises()
    expect(video.attributes('src')).toBe(`blob:http://localhost/0b6c${FIRST_FRAME}`)
  })

  it('streams straight from the server with a token for its file in the app', async () => {
    files.direct = { url: DIRECT, path: PATH }
    const video = player(PROXIED).find('video')
    await flushPromises()
    expect(video.attributes('src')).toBe(`${DIRECT}?token=t1${FIRST_FRAME}`)
    expect(files.getFileToken).toHaveBeenCalledWith(PATH)
  })

  /** Fail the current load with a media error of `code`. */
  async function fail(video: ReturnType<ReturnType<typeof player>['find']>, code: number) {
    Object.defineProperty(video.element, 'error', { value: { code }, configurable: true })
    await video.trigger('error')
    await flushPromises()
  }

  it('re-mints after a network failure, a few times, then falls back to the proxy', async () => {
    files.direct = { url: DIRECT, path: PATH }
    const video = player(PROXIED).find('video')
    await flushPromises()

    // A refused range (an expired token): a fresh token, same video. It
    // resumes where it stopped, so it has no first-frame fragment.
    await fail(video, MEDIA_ERR_NETWORK)
    expect(files.discardFileToken).toHaveBeenCalledTimes(1)
    expect(files.discardFileToken).toHaveBeenCalledWith(PATH)
    expect(video.attributes('src')).toBe(`${DIRECT}?token=t2`)

    await fail(video, MEDIA_ERR_NETWORK)
    await fail(video, MEDIA_ERR_NETWORK)
    expect(video.attributes('src')).toBe(`${DIRECT}?token=t4`)

    // One too many: back through the app.
    await fail(video, MEDIA_ERR_NETWORK)
    expect(video.attributes('src')).toBe(PROXIED)

    // A failure through the proxy is left to the player.
    await fail(video, MEDIA_ERR_NETWORK)
    expect(video.attributes('src')).toBe(PROXIED)
    expect(files.minted).toBe(4)
  })

  it('falls back to the proxy at once when the failure is not the network', async () => {
    files.direct = { url: DIRECT, path: PATH }
    const video = player(PROXIED).find('video')
    await flushPromises()

    await fail(video, MEDIA_ERR_DECODE)
    expect(video.attributes('src')).toBe(PROXIED)
    expect(files.minted).toBe(1)
  })

  it('falls back to the proxy when its file token cannot be minted', async () => {
    files.direct = { url: DIRECT, path: PATH }
    files.getFileToken.mockRejectedValue(new Error('refused'))
    const video = player(PROXIED).find('video')
    await flushPromises()
    // Still a first load, so from its first frame (the proxy reads only the
    // path and query).
    expect(video.attributes('src')).toBe(`${PROXIED}${FIRST_FRAME}`)
  })

  it('resumes a re-minted load where the failed one stopped', async () => {
    files.direct = { url: DIRECT, path: PATH }
    const video = player(PROXIED).find('video')
    await flushPromises()

    // jsdom doesn't play media: stand in for the element's clock.
    let time = 42
    const seeks: number[] = []
    Object.defineProperty(video.element, 'currentTime', {
      configurable: true,
      get: () => time,
      set: (next: number) => {
        time = next
        seeks.push(next)
      },
    })
    await fail(video, MEDIA_ERR_NETWORK)
    expect(video.attributes('src')).toBe(`${DIRECT}?token=t2`)

    time = 0
    await video.trigger('loadedmetadata')
    expect(seeks).toEqual([42])

    // Resumed once only.
    await video.trigger('loadedmetadata')
    expect(seeks).toEqual([42])
  })

  it('swaps a re-minted URL in without a fragment, and gives up when the token is the same', async () => {
    files.direct = { url: DIRECT, path: PATH }
    files.getFileToken.mockImplementation(async () => 'same')
    const video = player(PROXIED).find('video')
    await flushPromises()
    expect(video.attributes('src')).toBe(`${DIRECT}?token=same${FIRST_FRAME}`)

    // The fragment lives only on the element, so the re-mint still sees the
    // same URL and goes back through the app, resuming there.
    await fail(video, MEDIA_ERR_NETWORK)
    expect(files.getFileToken).toHaveBeenCalledTimes(2)
    expect(video.attributes('src')).toBe(PROXIED)
  })

  it('starts a new video from its first frame even mid-recovery', async () => {
    files.direct = { url: DIRECT, path: PATH }
    const wrapper = player(PROXIED)
    const video = wrapper.find('video')
    await flushPromises()
    await fail(video, MEDIA_ERR_NETWORK)

    files.direct = null
    await wrapper.setProps({ src: '/api/files/tickets/7/other.mp4' })
    await flushPromises()
    expect(video.attributes('src')).toBe(`/api/files/tickets/7/other.mp4${FIRST_FRAME}`)
  })
})

describe('VideoPlayer controls', () => {
  it("uses the browser's own controls on a touch device, and nothing of its own", async () => {
    pointer(true)
    const wrapper = player('/api/files/tickets/7/clip.mp4')
    await flushPromises()
    const video = wrapper.find('video')
    expect(video.attributes('controls')).toBeDefined()
    expect(video.attributes('playsinline')).toBeDefined()
    expect(video.attributes('x-webkit-airplay')).toBe('allow')
    expect(wrapper.find('button').exists()).toBe(false)
    expect(wrapper.find('input[type="range"]').exists()).toBe(false)

    // A tap on the picture is the native controls' to handle.
    const play = vi.fn(async () => {})
    Object.defineProperty(video.element, 'play', { value: play, configurable: true })
    await video.trigger('click')
    expect(play).not.toHaveBeenCalled()
  })

  it('keeps its own controls for a mouse', async () => {
    pointer(false)
    const wrapper = player('/api/files/tickets/7/clip.mp4')
    await flushPromises()
    const video = wrapper.find('video')
    expect(video.attributes('controls')).toBeUndefined()
    expect(video.attributes('playsinline')).toBeDefined()
    expect(wrapper.find('button[aria-label="Play"]').exists()).toBe(true)
    expect(wrapper.find('input[type="range"]').exists()).toBe(true)
    expect(wrapper.find('button[aria-label="Enter fullscreen"]').exists()).toBe(true)
    // The bar is as wide as the picture: a portrait box sheds the slider.
    expect(wrapper.classes()).toContain('@container')
    expect(wrapper.find('input[type="range"]').classes()).toContain('@min-[20rem]:block')
  })

  it('holds the native controls back while the first frame primes', async () => {
    pointer(true)
    let settle: (() => void) | undefined
    priming.prime = (_video, onSettled) => {
      settle = onSettled
      return () => {}
    }
    const wrapper = player('/api/files/tickets/7/clip.mp4')
    await flushPromises()
    expect(settle).toBeDefined()
    expect(wrapper.find('video').attributes('controls')).toBeUndefined()

    settle!()
    await nextTick()
    expect(wrapper.find('video').attributes('controls')).toBeDefined()
  })

  it('hands the native controls over when the first frame stalls', async () => {
    vi.useFakeTimers()
    try {
      pointer(true)
      let settle: (() => void) | undefined
      const cancel = vi.fn(() => settle?.())
      priming.prime = (_video, onSettled) => {
        settle = onSettled
        return cancel
      }
      const wrapper = player('/api/files/tickets/7/clip.mp4')
      await flushPromises()
      const pause = vi.fn()
      Object.defineProperty(wrapper.find('video').element, 'pause', { value: pause, configurable: true })
      expect(wrapper.find('video').attributes('controls')).toBeUndefined()

      vi.advanceTimersByTime(4000)
      await nextTick()
      expect(cancel).toHaveBeenCalled()
      expect(pause).toHaveBeenCalled()
      expect(wrapper.find('video').attributes('controls')).toBeDefined()
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('VideoPlayer shape', () => {
  /** Give the element a picture of `width` by `height`; jsdom decodes none. */
  function sized(video: HTMLVideoElement, width: number, height: number) {
    Object.defineProperty(video, 'videoWidth', { value: width, configurable: true })
    Object.defineProperty(video, 'videoHeight', { value: height, configurable: true })
  }

  it('holds a 16:9 placeholder, then takes the shape of the video', async () => {
    const wrapper = player('/api/files/tickets/7/clip.mp4')
    await flushPromises()
    expect(wrapper.attributes('style')).toContain('aspect-ratio: 16 / 9')
    expect(wrapper.attributes('style')).toContain('max-height: 16rem')
    expect(wrapper.classes()).not.toContain('min-w-[300px]')

    const video = wrapper.find('video')
    sized(video.element as HTMLVideoElement, 1080, 1920)
    await video.trigger('loadedmetadata')
    const style = wrapper.attributes('style') ?? ''
    expect(style).toContain('aspect-ratio: 1080 / 1920')
    // The width is capped through the ratio as well, so a portrait video
    // gets a portrait box, not a wide one with bars.
    expect(style).toMatch(/max-width: calc\(/)
  })

  it('keeps the placeholder for a file with no picture', async () => {
    const wrapper = player('/api/files/tickets/7/clip.mp4')
    await flushPromises()
    const video = wrapper.find('video')
    sized(video.element as HTMLVideoElement, 0, 0)
    await video.trigger('loadedmetadata')
    expect(wrapper.attributes('style')).toContain('aspect-ratio: 16 / 9')
  })

  it('takes the height its caller allows', async () => {
    const wrapper = player('/api/files/tickets/7/clip.mp4', { maxHeight: '70dvh' })
    await flushPromises()
    expect(wrapper.attributes('style')).toContain('max-height: 70dvh')
  })
})
