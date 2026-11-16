import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { configureAssetUrl, proxiedAssetResolver } from '@nosdesk/core/transport'

// Saving a file works on both hosts. The web hands it to the browser: a server
// file as a link the browser follows with its cookie, bytes through an object
// URL. The app's webview ignores a download link, so there the file goes to
// the share sheet: a server file downloaded natively by its stored path, bytes
// over IPC, one save at a time.
const mobile = vi.hoisted(() => ({
  shareServerFile: vi.fn(async () => ({ completed: true })),
  shareFile: vi.fn(async () => ({ completed: true })),
}))
vi.mock('@nosdesk/mobile', () => mobile)

const { saveFile, shareFile } = await import('./files')

const inApp = (on: boolean) => {
  ;(globalThis as { isTauri?: boolean }).isTauri = on
}

afterEach(() => {
  inApp(false)
  configureAssetUrl(
    (p) => p,
    (u) => u,
  )
  vi.restoreAllMocks()
  mobile.shareServerFile.mockClear()
  mobile.shareFile.mockClear()
})

describe('on the web', () => {
  let clicked: { href: string; download: string }[]
  beforeEach(() => {
    clicked = []
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      clicked.push({ href: this.getAttribute('href') ?? '', download: this.download })
    })
  })

  it('lets the browser fetch a server file, named by the caller or the server', async () => {
    await expect(saveFile('/api/files/tickets/1/a.wav', 'Voice note.wav')).resolves.toEqual({ completed: true })
    await saveFile('/api/admin/backup/download/7')
    expect(clicked).toEqual([
      { href: '/api/files/tickets/1/a.wav', download: 'Voice note.wav' },
      { href: '/api/admin/backup/download/7', download: '' },
    ])
    // A legacy stored path goes to the authenticated route.
    await saveFile('/uploads/tickets/1/b.pdf', 'b.pdf')
    expect(clicked[2].href).toBe('/api/files/tickets/1/b.pdf')
    expect(document.querySelectorAll('a')).toHaveLength(0)
    expect(mobile.shareServerFile).not.toHaveBeenCalled()
  })

  it('saves bytes through an object URL it revokes afterwards', async () => {
    vi.useFakeTimers()
    URL.createObjectURL = vi.fn(() => 'blob:report')
    URL.revokeObjectURL = vi.fn()
    await saveFile(new Blob(['a,b'], { type: 'text/csv' }), 'report.csv')
    expect(clicked).toEqual([{ href: 'blob:report', download: 'report.csv' }])
    expect(URL.revokeObjectURL).not.toHaveBeenCalled()
    vi.runAllTimers()
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:report')
    vi.useRealTimers()
  })

  it('has no share sheet to hand bytes to', async () => {
    await expect(shareFile(new Uint8Array([1]), 'a.pdf', 'application/pdf')).rejects.toThrow()
  })
})

describe('in the app', () => {
  beforeEach(() => {
    inApp(true)
    configureAssetUrl(proxiedAssetResolver('nosdesk-asset://localhost'), (url) =>
      url.replace(/^nosdesk-asset:\/\/localhost/, ''),
    )
  })

  it('downloads a server file natively by its stored path', async () => {
    await saveFile('/api/files/tickets/1/a.wav', 'Voice note.wav')
    // A URL already resolved for display goes back to the stored path.
    await saveFile('nosdesk-asset://localhost/api/files/tickets/1/b.mp4')
    await saveFile('/uploads/tickets/1/c.pdf')
    expect(mobile.shareServerFile.mock.calls).toEqual([
      ['/api/files/tickets/1/a.wav', 'Voice note.wav'],
      ['/api/files/tickets/1/b.mp4', undefined],
      ['/api/files/tickets/1/c.pdf', undefined],
    ])
  })

  it('hands bytes to the share sheet with their type', async () => {
    await saveFile(new Blob(['a,b'], { type: 'text/csv' }), 'report.csv')
    const [bytes, name, type] = mobile.shareFile.mock.calls[0] as unknown as [Uint8Array, string, string]
    expect(Array.from(bytes)).toEqual([97, 44, 98])
    expect([name, type]).toEqual(['report.csv', 'text/csv'])
  })

  it('reads a file not uploaded yet from its blob URL', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response('x', { headers: { 'Content-Type': 'audio/wav' } }),
    )
    await saveFile('blob:tauri://localhost/123', 'voice.wav')
    expect(fetch).toHaveBeenCalledWith('blob:tauri://localhost/123')
    expect(mobile.shareFile.mock.calls[0].slice(1)).toEqual(['voice.wav', 'audio/wav'])
    expect(mobile.shareServerFile).not.toHaveBeenCalled()
  })

  it('refuses a URL that is not a file on the server', async () => {
    await expect(saveFile('https://elsewhere.example/a.pdf')).rejects.toThrow(/not a server file/)
    await expect(saveFile('/assets/logo.svg')).rejects.toThrow(/not a server file/)
    expect(mobile.shareServerFile).not.toHaveBeenCalled()
  })

  it('reports a dismissed sheet', async () => {
    mobile.shareServerFile.mockResolvedValueOnce({ completed: false })
    await expect(saveFile('/api/files/tickets/1/a.wav')).resolves.toEqual({ completed: false })
  })

  it('runs one save at a time, and a failure does not hold up the next', async () => {
    let finishFirst!: () => void
    mobile.shareServerFile.mockImplementationOnce(
      () => new Promise((resolve) => (finishFirst = () => resolve({ completed: true }))),
    )
    mobile.shareServerFile.mockRejectedValueOnce(new Error('403'))
    const first = saveFile('/api/files/1')
    const second = saveFile('/api/files/2')
    const third = shareFile(new Uint8Array([1]), 'a.pdf', 'application/pdf')
    await vi.waitFor(() => expect(mobile.shareServerFile).toHaveBeenCalledTimes(1))
    expect(mobile.shareFile).not.toHaveBeenCalled()

    finishFirst()
    await expect(first).resolves.toEqual({ completed: true })
    await expect(second).rejects.toThrow('403')
    await expect(third).resolves.toEqual({ completed: true })
  })
})
