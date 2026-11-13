import { afterEach, describe, expect, it, vi } from 'vitest'

// A file too big to upload is refused from its size alone, before any byte of
// it is read: at the pickers, against the server's limit and any transport cap.
// The mobile app's HTTP adapter sends file bodies through the webview's XHR,
// which streams them from disk, rather than its native bridge, which copied a
// phone video through memory until iOS killed the app.
const http = vi.hoisted(() => ({ fetch: vi.fn(), xhr: vi.fn() }))
vi.mock('../../../mobile/node_modules/@tauri-apps/plugin-http/dist-js/index.js', () => ({
  fetch: http.fetch,
}))
vi.mock('axios', async (importOriginal) => {
  const actual = await importOriginal<typeof import('axios')>()
  const getAdapter = (name: unknown) => (name === 'xhr' ? http.xhr : actual.default.getAdapter(name as never))
  return { ...actual, default: Object.assign(actual.default, { getAdapter }) }
})
const api = vi.hoisted(() => ({ get: vi.fn() }))
vi.mock('@nosdesk/core/apiClient', () => ({ default: api }))

const MB = 1024 * 1024
/** A file of `size` bytes; jsdom keeps the bytes, so sizes stay modest. */
const fileOf = (size: number, name = 'clip.mov') =>
  new File([new Uint8Array(size)], name, { type: 'video/quicktime' })

const limits = await import('@nosdesk/core/services/uploadLimits')
const config = await import('@nosdesk/core/services/instanceConfig')
const { AxiosHeaders } = await import('axios')
const { tauriHttpAdapter } = await import('@nosdesk/mobile/tauriHttpAdapter')

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
  http.xhr.mockReset()
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
  const send = (data: unknown) =>
    tauriHttpAdapter({
      url: '/upload',
      baseURL: 'https://help.example.com/api',
      method: 'post',
      data,
      headers: new AxiosHeaders({
        Authorization: 'Bearer t',
        'X-Auth-Mode': 'bearer',
        'X-Nosdesk-Workspace': 'acme',
        'X-Correlation-ID': 'c',
        'X-Nosdesk-Trace-Id': 't',
        'X-SSE-Client-Id': 's',
      }),
    } as never)

  it('sends a file body through the XHR, whatever its size, with only the headers the API allows cross-origin', async () => {
    http.xhr.mockResolvedValue({ status: 200, data: [] })
    const video = fileOf(1)
    Object.defineProperty(video, 'size', { value: 500 * MB })
    const form = new FormData()
    form.append('files', video)

    for (const body of [form, video]) await send(body)

    expect(http.fetch).not.toHaveBeenCalled()
    expect(http.xhr).toHaveBeenCalledTimes(2)
    const sent = http.xhr.mock.calls[0][0].headers.toJSON()
    expect(Object.keys(sent).sort()).toEqual(['Authorization', 'X-Auth-Mode', 'X-Nosdesk-Workspace'])
  })

  it('sends any other body through the native client, headers and all', async () => {
    http.fetch.mockResolvedValue(new Response('{}', { status: 200 }))
    const res = await send('{"a":1}')
    expect(res.status).toBe(200)
    expect(http.xhr).not.toHaveBeenCalled()
    const init = http.fetch.mock.calls[0][1] as RequestInit
    expect(init.headers).toMatchObject({ 'X-Correlation-ID': 'c', 'X-Nosdesk-Workspace': 'acme' })
  })
})
