import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'

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

import VideoPlayer from '../VideoPlayer.vue'

function player(src: string) {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  return mount(VideoPlayer, {
    props: { src },
    global: { plugins: [createFluentVue({ bundles: [bundle] })] },
  })
}

beforeEach(() => {
  files.direct = null
  files.minted = 0
  files.getFileToken.mockReset()
  files.getFileToken.mockImplementation(async () => `t${++files.minted}`)
  files.discardFileToken.mockClear()
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
