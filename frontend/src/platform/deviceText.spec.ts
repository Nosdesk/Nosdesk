import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const native = vi.hoisted(() => ({
  getDeviceText: vi.fn(),
  followDeviceText: vi.fn(),
  onDeviceTextChange: vi.fn(),
  changeHandler: null as ((settings: unknown) => void) | null,
}))
vi.mock('@nosdesk/mobile/deviceText', () => ({
  getDeviceText: native.getDeviceText,
  followDeviceText: native.followDeviceText,
  onDeviceTextChange: native.onDeviceTextChange,
}))

const root = document.documentElement
let loaded: typeof import('./deviceText') | null = null

async function freshModule() {
  vi.resetModules()
  loaded = await import('./deviceText')
  return loaded
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((r) => (resolve = r))
  return { promise, resolve }
}

beforeEach(() => {
  native.changeHandler = null
  native.getDeviceText.mockReset()
  native.followDeviceText.mockReset().mockResolvedValue(undefined)
  native.onDeviceTextChange.mockReset().mockImplementation(async (handler: (settings: unknown) => void) => {
    native.changeHandler = handler
    return () => {}
  })
})

afterEach(() => {
  loaded?.stopDeviceText()
  loaded = null
  delete (globalThis as { isTauri?: boolean }).isTauri
  root.style.removeProperty('--device-text-scale')
  root.removeAttribute('data-bold-text')
  vi.useRealTimers()
})

describe('parseDeviceText', () => {
  it('reads a plugin payload, to 4 decimals', async () => {
    const { parseDeviceText } = await freshModule()
    expect(parseDeviceText({ scale: 19 / 17, bold: true })).toEqual({ scale: 1.1176, bold: true })
  })

  it('passes sizes below the default through', async () => {
    const { parseDeviceText } = await freshModule()
    expect(parseDeviceText({ scale: 14 / 17, bold: false })?.scale).toBe(0.8235)
  })

  it('bounds the scale to 0.5 and 2', async () => {
    const { parseDeviceText } = await freshModule()
    expect(parseDeviceText({ scale: 53 / 17, bold: false })?.scale).toBe(2)
    expect(parseDeviceText({ scale: 0.1, bold: false })?.scale).toBe(0.5)
  })

  it('rejects anything that is not settings', async () => {
    const { parseDeviceText } = await freshModule()
    for (const raw of [null, undefined, 1.2, 'big', {}, { scale: '1.2', bold: false }, { scale: NaN, bold: false }, { scale: 1.2 }]) {
      expect(parseDeviceText(raw)).toBeNull()
    }
  })
})

describe('applyDeviceText', () => {
  it('writes the scale as a unitless number and marks Bold Text', async () => {
    const { applyDeviceText } = await freshModule()
    applyDeviceText({ scale: 1.1176, bold: true })
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('1.1176')
    expect(root.getAttribute('data-bold-text')).toBe('true')
  })

  it('removes the Bold Text attribute when it is off', async () => {
    const { applyDeviceText } = await freshModule()
    applyDeviceText({ scale: 1, bold: true })
    applyDeviceText({ scale: 1, bold: false })
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('1')
    expect(root.hasAttribute('data-bold-text')).toBe(false)
  })
})

describe('initDeviceText in a browser', () => {
  it('leaves the page alone and never loads the native module', async () => {
    const { initDeviceText, deviceText } = await freshModule()
    await initDeviceText({ follow: true })
    expect(native.getDeviceText).not.toHaveBeenCalled()
    expect(native.followDeviceText).not.toHaveBeenCalled()
    expect(native.onDeviceTextChange).not.toHaveBeenCalled()
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('')
    expect(root.hasAttribute('data-bold-text')).toBe(false)
    expect(deviceText.value).toBeNull()
  })
})

describe('initDeviceText in the app, not following', () => {
  beforeEach(() => {
    ;(globalThis as { isTauri?: boolean }).isTauri = true
  })

  it('reads and exposes the settings without touching the page or the webview', async () => {
    native.getDeviceText.mockResolvedValue({ scale: 19 / 17, bold: true })
    const { initDeviceText, deviceText } = await freshModule()
    await initDeviceText({ follow: false })
    expect(deviceText.value).toEqual({ scale: 1.1176, bold: true })
    native.changeHandler!({ scale: 28 / 17, bold: false })
    expect(deviceText.value).toEqual({ scale: 1.6471, bold: false })
    expect(native.followDeviceText).not.toHaveBeenCalled()
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('')
    expect(root.hasAttribute('data-bold-text')).toBe(false)
  })
})

describe('initDeviceText in the app, following', () => {
  beforeEach(() => {
    ;(globalThis as { isTauri?: boolean }).isTauri = true
  })

  it('is the default', async () => {
    const { FOLLOW_DEVICE_TEXT } = await freshModule()
    expect(FOLLOW_DEVICE_TEXT).toBe(true)
  })

  it('takes over from the webview, then applies the settings before it resolves', async () => {
    const order: string[] = []
    native.followDeviceText.mockImplementation(async () => void order.push('follow'))
    native.getDeviceText.mockImplementation(async () => {
      order.push('get')
      return { scale: 19 / 17, bold: true }
    })
    const { initDeviceText, deviceText } = await freshModule()
    await initDeviceText({ follow: true })
    expect(order).toEqual(['follow', 'get'])
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('1.1176')
    expect(root.getAttribute('data-bold-text')).toBe('true')
    expect(deviceText.value).toEqual({ scale: 1.1176, bold: true })
  })

  it('leaves the page alone when the plugin cannot take over', async () => {
    native.followDeviceText.mockRejectedValue(new Error('no follow'))
    native.getDeviceText.mockResolvedValue({ scale: 1.2, bold: true })
    const { initDeviceText, deviceText } = await freshModule()
    await initDeviceText({ follow: true })
    expect(deviceText.value).toEqual({ scale: 1.2, bold: true })
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('')
    expect(root.hasAttribute('data-bold-text')).toBe(false)
  })

  it('follows a change made while the app is open', async () => {
    native.getDeviceText.mockResolvedValue({ scale: 1, bold: false })
    const { initDeviceText, deviceText } = await freshModule()
    await initDeviceText({ follow: true })
    native.changeHandler!({ scale: 28 / 17, bold: true })
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('1.6471')
    expect(root.getAttribute('data-bold-text')).toBe('true')
    expect(deviceText.value).toEqual({ scale: 1.6471, bold: true })
  })

  it('treats a repeated change as nothing new', async () => {
    native.getDeviceText.mockResolvedValue({ scale: 1, bold: false })
    const { initDeviceText, deviceText } = await freshModule()
    await initDeviceText({ follow: true })
    native.changeHandler!({ scale: 1.2, bold: false })
    const first = deviceText.value
    root.style.setProperty('--device-text-scale', 'sentinel')
    native.changeHandler!({ scale: 1.2, bold: false })
    expect(deviceText.value).toBe(first)
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('sentinel')
  })

  it('reads again when the app comes back to the foreground', async () => {
    native.getDeviceText.mockResolvedValue({ scale: 1, bold: false })
    const { initDeviceText } = await freshModule()
    await initDeviceText({ follow: true })
    native.getDeviceText.mockResolvedValue({ scale: 23 / 17, bold: false })
    document.dispatchEvent(new Event('visibilitychange'))
    await vi.waitFor(() => expect(root.style.getPropertyValue('--device-text-scale')).toBe('1.3529'))
    expect(native.getDeviceText).toHaveBeenCalledTimes(2)
  })

  it('drops a read that a change event overtook', async () => {
    native.getDeviceText.mockResolvedValue({ scale: 1, bold: false })
    const { initDeviceText, deviceText } = await freshModule()
    await initDeviceText({ follow: true })
    const late = deferred<unknown>()
    native.getDeviceText.mockReturnValue(late.promise)
    document.dispatchEvent(new Event('visibilitychange'))
    native.changeHandler!({ scale: 28 / 17, bold: false })
    late.resolve({ scale: 1, bold: false })
    await late.promise
    await Promise.resolve()
    expect(deviceText.value).toEqual({ scale: 1.6471, bold: false })
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('1.6471')
  })

  it('stops reading on foreground once stopped', async () => {
    native.getDeviceText.mockResolvedValue({ scale: 1, bold: false })
    const { initDeviceText, stopDeviceText } = await freshModule()
    await initDeviceText({ follow: true })
    stopDeviceText()
    document.dispatchEvent(new Event('visibilitychange'))
    expect(native.getDeviceText).toHaveBeenCalledTimes(1)
  })

  it('ignores a payload that is not settings', async () => {
    native.getDeviceText.mockResolvedValue({ scale: 1.2, bold: false })
    const { initDeviceText, deviceText } = await freshModule()
    await initDeviceText({ follow: true })
    native.changeHandler!({ scale: 'huge' })
    expect(deviceText.value).toEqual({ scale: 1.2, bold: false })
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('1.2')
  })

  it('starts the app at the default size when the plugin fails', async () => {
    native.followDeviceText.mockRejectedValue(new Error('plugin device-text not found'))
    native.getDeviceText.mockRejectedValue(new Error('plugin device-text not found'))
    native.onDeviceTextChange.mockRejectedValue(new Error('plugin device-text not found'))
    const { initDeviceText, deviceText } = await freshModule()
    await expect(initDeviceText({ follow: true })).resolves.toBeUndefined()
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('')
    expect(deviceText.value).toBeNull()
  })

  it('does not hold launch for a plugin that never answers', async () => {
    vi.useFakeTimers()
    native.followDeviceText.mockReturnValue(new Promise(() => {}))
    const { initDeviceText } = await freshModule()
    let done = false
    const init = initDeviceText({ follow: true }).then(() => {
      done = true
    })
    await vi.advanceTimersByTimeAsync(499)
    expect(done).toBe(false)
    await vi.advanceTimersByTimeAsync(1)
    await init
    expect(done).toBe(true)
  })
})
