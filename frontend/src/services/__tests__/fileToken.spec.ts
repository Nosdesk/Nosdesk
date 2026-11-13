import { beforeEach, describe, expect, it, vi } from 'vitest'

let calls = 0
let failNext = 0
/** The path each mint asked for, in order. */
let mintedFor: string[] = []
vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    post: vi.fn(async (url: string, body?: { path?: string }) => {
      calls++
      if (url !== '/files/token') throw new Error(`unexpected ${url}`)
      if (!body?.path) throw new Error('no path')
      mintedFor.push(body.path)
      if (failNext > 0) {
        failNext--
        throw new Error('network down')
      }
      return { data: { token: `t${calls}`, expires_in: 900, refresh_buffer: 600 } }
    }),
  },
}))

const env = vi.hoisted(() => ({
  tauri: true,
  fileTokens: true,
  apiBase: 'https://help.example.com/api',
}))
vi.mock('@/platform', () => ({ isTauriRuntime: () => env.tauri }))
vi.mock('@nosdesk/core/services/instanceConfig', () => ({
  serverMintsFileTokens: () => env.fileTokens,
}))
vi.mock('@nosdesk/core/transport', () => ({
  apiBaseUrl: () => env.apiBase,
  // The app's proxy prefix, stripped as the mobile reverse resolver does.
  assetPath: (url: string) => url.replace(/^nosdesk-asset:\/\/localhost/, ''),
}))

import {
  directVideoUrl,
  discardFileToken,
  getFileToken,
  resetFileToken,
  withFileToken,
} from '@/services/fileToken'

const PROXIED = 'nosdesk-asset://localhost/api/files/tickets/42/clip.mp4'
const CLIP = '/api/files/tickets/42/clip.mp4'
const OTHER = '/api/files/tickets/42/other.mov'

beforeEach(() => {
  calls = 0
  failNext = 0
  mintedFor = []
  env.tauri = true
  env.fileTokens = true
  env.apiBase = 'https://help.example.com/api'
  resetFileToken()
  vi.useRealTimers()
})

describe('file token', () => {
  it('is minted for the path it opens', async () => {
    expect(await getFileToken(CLIP)).toBe('t1')
    expect(mintedFor).toEqual([CLIP])
  })

  it('shares one fetch between concurrent callers for a path', async () => {
    const [a, b] = await Promise.all([getFileToken(CLIP), getFileToken(CLIP)])
    expect(a).toBe('t1')
    expect(b).toBe('t1')
    expect(calls).toBe(1)
  })

  it('is minted separately for each path, and cached per path', async () => {
    const [a, b] = await Promise.all([getFileToken(CLIP), getFileToken(OTHER)])
    expect(a).toBe('t1')
    expect(b).toBe('t2')
    expect(mintedFor).toEqual([CLIP, OTHER])
    expect(await getFileToken(CLIP)).toBe('t1')
    expect(await getFileToken(OTHER)).toBe('t2')
    expect(calls).toBe(2)
  })

  it('is reused until its refresh point, then re-minted', async () => {
    vi.useFakeTimers()
    expect(await getFileToken(CLIP)).toBe('t1')
    // 900s TTL less the 600s buffer.
    vi.advanceTimersByTime(299_000)
    expect(await getFileToken(CLIP)).toBe('t1')
    vi.advanceTimersByTime(2_000)
    expect(await getFileToken(CLIP)).toBe('t2')
  })

  it('is re-minted after a load refused it, for that path only', async () => {
    expect(await getFileToken(CLIP)).toBe('t1')
    expect(await getFileToken(OTHER)).toBe('t2')
    discardFileToken(CLIP)
    expect(await getFileToken(CLIP)).toBe('t3')
    expect(await getFileToken(OTHER)).toBe('t2')
  })

  it('is dropped for every path on a workspace switch or sign-out', async () => {
    expect(await getFileToken(CLIP)).toBe('t1')
    expect(await getFileToken(OTHER)).toBe('t2')
    resetFileToken()
    expect(await getFileToken(CLIP)).toBe('t3')
    expect(await getFileToken(OTHER)).toBe('t4')
  })

  it('is not cached when the reset lands mid-fetch', async () => {
    const before = getFileToken(CLIP)
    resetFileToken()
    expect(await before).toBe('t1')
    // The fetch that finished after the reset wasn't cached.
    expect(await getFileToken(CLIP)).toBe('t2')
  })

  it('retries after a failed mint', async () => {
    failNext = 1
    await expect(getFileToken(CLIP)).rejects.toThrow('network down')
    expect(await getFileToken(CLIP)).toBe('t2')
  })
})

describe('the URL a video streams from', () => {
  it('is the server URL in the app, on a server that mints file tokens, over https', () => {
    expect(directVideoUrl(PROXIED)).toEqual({
      url: 'https://help.example.com/api/files/tickets/42/clip.mp4',
      path: CLIP,
    })
  })

  it('names the path the element loads, as the app stored it', () => {
    const direct = directVideoUrl('nosdesk-asset://localhost/api/files/temp/0190_Site visit é.mov')
    expect(direct?.path).toBe('/api/files/temp/0190_Site visit é.mov')
    expect(direct?.url).toBe(`https://help.example.com${direct?.path}`)
  })

  it('stays the proxy on a server that predates file tokens', () => {
    env.fileTokens = false
    expect(directVideoUrl(PROXIED)).toBeNull()
  })

  it('stays the proxy on a plain http server', () => {
    env.apiBase = 'http://192.168.1.20:8080/api'
    expect(directVideoUrl(PROXIED)).toBeNull()
  })

  it('is left alone outside the app', () => {
    env.tauri = false
    expect(directVideoUrl('/api/files/tickets/42/clip.mp4')).toBeNull()
  })

  it('is left alone for a local or non-file source', () => {
    expect(directVideoUrl('blob:tauri://localhost/123')).toBeNull()
    expect(directVideoUrl('nosdesk-asset://localhost/uploads/users/avatars/a.png')).toBeNull()
  })

  it('carries the token as a query parameter', () => {
    expect(withFileToken('https://h/api/files/a.mp4', 'x.y-z')).toBe(
      'https://h/api/files/a.mp4?token=x.y-z',
    )
    expect(withFileToken('https://h/api/files/a.mp4?v=2', 'a+b')).toBe(
      'https://h/api/files/a.mp4?v=2&token=a%2Bb',
    )
  })
})
