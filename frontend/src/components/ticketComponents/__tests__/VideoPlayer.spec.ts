import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'

const PROXIED = 'nosdesk-asset://localhost/api/files/tickets/7/clip.mp4'
const PATH = '/api/files/tickets/7/clip.mp4'
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
  it('plays the given source where there is no direct URL', async () => {
    const video = player('/api/files/tickets/7/clip.mp4').find('video')
    await flushPromises()
    expect(video.attributes('src')).toBe('/api/files/tickets/7/clip.mp4')
  })

  it('streams straight from the server with a token for its file in the app', async () => {
    files.direct = { url: DIRECT, path: PATH }
    const video = player(PROXIED).find('video')
    await flushPromises()
    expect(video.attributes('src')).toBe(`${DIRECT}?token=t1`)
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

    // A refused range (an expired token): a fresh token, same video.
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
    expect(video.attributes('src')).toBe(PROXIED)
  })
})
