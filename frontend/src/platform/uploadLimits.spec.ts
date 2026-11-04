import { afterEach, describe, expect, it, vi } from 'vitest'

// A file too big to upload is refused from its size alone, before any byte of
// it is read: at the pickers, against the server's limit and the mobile app's
// transport cap, and again in the app's HTTP adapter for every other path. A
// phone video sent through the adapter's native bridge got the app killed.
const http = vi.hoisted(() => ({ fetch: vi.fn() }))
vi.mock('../../../mobile/node_modules/@tauri-apps/plugin-http/dist-js/index.js', () => ({
  fetch: http.fetch,
}))
const api = vi.hoisted(() => ({ get: vi.fn() }))
vi.mock('@nosdesk/core/apiClient', () => ({ default: api }))

const MB = 1024 * 1024
/** A file of `size` bytes; jsdom keeps the bytes, so sizes stay modest. */
const fileOf = (size: number, name = 'clip.mov') =>
  new File([new Uint8Array(size)], name, { type: 'video/quicktime' })

const limits = await import('@nosdesk/core/services/uploadLimits')
const config = await import('@nosdesk/core/services/instanceConfig')
const { tauriHttpAdapter, BRIDGE_UPLOAD_CAP_BYTES } = await import(
  '@nosdesk/mobile/tauriHttpAdapter'
)

async function serverLimit(mb: number | undefined, blocked?: string[]) {
  config.resetInstanceConfig()
  api.get.mockResolvedValue({
    data: {
      workspace_routing: 'host',
      deployment_mode: 'hosted',
      max_file_size_mb: mb,
      blocked_file_extensions: blocked,
    },
  })
  await config.fetchInstanceConfig()
}

afterEach(() => {
  limits.setTransportUploadCap(null)
  config.resetInstanceConfig()
  http.fetch.mockReset()
  api.get.mockReset()
})

describe('upload limits', () => {
  it('refuses nothing while no limit is known', () => {
    expect(limits.uploadLimitBytes()).toBeNull()
    expect(limits.oversizeFiles([fileOf(2 * MB)])).toEqual([])
  })

  it("takes the server's limit from /api/config", async () => {
    await serverLimit(1)
    expect(limits.uploadLimitBytes()).toBe(MB)
    const small = fileOf(MB, 'small.png')
    const big = fileOf(MB + 1)
    expect(limits.oversizeFiles([small, big])).toEqual([big])
  })

  it('keeps no limit from a server that names none, and forgets it on reset', async () => {
    await serverLimit(undefined)
    expect(limits.uploadLimitBytes()).toBeNull()
    await serverLimit(50)
    config.resetInstanceConfig()
    expect(limits.uploadLimitBytes()).toBeNull()
  })

  it('refuses the file types the server refuses, by extension, whatever the case', async () => {
    const named = (name: string) => new File(['x'], name)
    await serverLimit(50, undefined)
    expect(limits.disallowedFiles([named('setup.exe')])).toEqual([])

    await serverLimit(50, ['exe', 'bin'])
    const files = [named('setup.EXE'), named('qa.bin'), named('notes.txt'), named('.bin'), named('bin')]
    expect(limits.disallowedFiles(files).map((f) => f.name)).toEqual(['setup.EXE', 'qa.bin'])
  })

  it("uses the transport's cap when it is lower than the server's", async () => {
    await serverLimit(50)
    limits.setTransportUploadCap(MB)
    expect(limits.uploadLimitBytes()).toBe(MB)
    limits.setTransportUploadCap(100 * MB)
    expect(limits.uploadLimitBytes()).toBe(50 * MB)
  })
})

describe('the mobile HTTP adapter', () => {
  const post = (data: unknown) =>
    tauriHttpAdapter({
      url: '/upload',
      baseURL: 'https://help.example.com/api',
      method: 'post',
      data,
      headers: { toJSON: () => ({}) },
    } as never)

  it('refuses a body over its cap without sending it', async () => {
    const form = new FormData()
    form.append('files', fileOf(BRIDGE_UPLOAD_CAP_BYTES / 2, 'a.mov'))
    form.append('files', fileOf(BRIDGE_UPLOAD_CAP_BYTES / 2 + 1, 'b.mov'))
    await expect(post(form)).rejects.toMatchObject({ code: limits.UPLOAD_TOO_LARGE })
    await expect(post(fileOf(BRIDGE_UPLOAD_CAP_BYTES + 1))).rejects.toMatchObject({
      code: limits.UPLOAD_TOO_LARGE,
    })
    expect(http.fetch).not.toHaveBeenCalled()
  })

  it('sends a body within it', async () => {
    http.fetch.mockResolvedValue(new Response('[]', { status: 200 }))
    const form = new FormData()
    form.append('files', fileOf(MB))
    const res = await post(form)
    expect(res.status).toBe(200)
    expect(http.fetch).toHaveBeenCalledOnce()
  })
})
