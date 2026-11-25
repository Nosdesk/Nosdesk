import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const native = vi.hoisted(() => ({
  getDeviceText: vi.fn(),
  onDeviceTextChange: vi.fn(),
  changeHandler: null as ((settings: unknown) => void) | null,
}))
vi.mock('@nosdesk/mobile/deviceText', () => ({
  getDeviceText: native.getDeviceText,
  onDeviceTextChange: native.onDeviceTextChange,
}))

const root = document.documentElement

async function freshModule() {
  vi.resetModules()
  return import('./deviceText')
}

beforeEach(() => {
  native.changeHandler = null
  native.getDeviceText.mockReset()
  native.onDeviceTextChange.mockReset().mockImplementation(async (handler: (settings: unknown) => void) => {
    native.changeHandler = handler
    return () => {}
  })
})

afterEach(() => {
  delete (globalThis as { isTauri?: boolean }).isTauri
  root.style.removeProperty('--device-text-scale')
  root.removeAttribute('data-bold-text')
  vi.useRealTimers()
})

describe('parseDeviceText', () => {
  it('reads a plugin payload', async () => {
    const { parseDeviceText } = await freshModule()
    expect(parseDeviceText({ scale: 19 / 17, bold: true })).toEqual({ scale: 19 / 17, bold: true })
  })

  it('passes sizes below the default through', async () => {
    const { parseDeviceText } = await freshModule()
    expect(parseDeviceText({ scale: 14 / 17, bold: false })?.scale).toBe(14 / 17)
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
    applyDeviceText({ scale: 19 / 17, bold: true })
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
    await initDeviceText()
    expect(native.getDeviceText).not.toHaveBeenCalled()
    expect(native.onDeviceTextChange).not.toHaveBeenCalled()
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('')
    expect(root.hasAttribute('data-bold-text')).toBe(false)
    expect(deviceText.value).toBeNull()
  })
})

describe('initDeviceText in the app', () => {
  beforeEach(() => {
    ;(globalThis as { isTauri?: boolean }).isTauri = true
  })

  it('applies the settings before it resolves', async () => {
    native.getDeviceText.mockResolvedValue({ scale: 19 / 17, bold: true })
    const { initDeviceText, deviceText } = await freshModule()
    await initDeviceText()
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('1.1176')
    expect(root.getAttribute('data-bold-text')).toBe('true')
    expect(deviceText.value).toEqual({ scale: 19 / 17, bold: true })
  })

  it('follows a change made while the app is open', async () => {
    native.getDeviceText.mockResolvedValue({ scale: 1, bold: false })
    const { initDeviceText, deviceText } = await freshModule()
    await initDeviceText()
    native.changeHandler!({ scale: 28 / 17, bold: true })
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('1.6471')
    expect(root.getAttribute('data-bold-text')).toBe('true')
    expect(deviceText.value).toEqual({ scale: 28 / 17, bold: true })
  })

  it('reads again when the app comes back to the foreground', async () => {
    native.getDeviceText.mockResolvedValue({ scale: 1, bold: false })
    const { initDeviceText } = await freshModule()
    await initDeviceText()
    native.getDeviceText.mockResolvedValue({ scale: 23 / 17, bold: false })
    document.dispatchEvent(new Event('visibilitychange'))
    await vi.waitFor(() => expect(root.style.getPropertyValue('--device-text-scale')).toBe('1.3529'))
  })

  it('ignores a payload that is not settings', async () => {
    native.getDeviceText.mockResolvedValue({ scale: 1.2, bold: false })
    const { initDeviceText, deviceText } = await freshModule()
    await initDeviceText()
    native.changeHandler!({ scale: 'huge' })
    expect(deviceText.value).toEqual({ scale: 1.2, bold: false })
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('1.2')
  })

  it('starts the app at the default size when the plugin fails', async () => {
    native.getDeviceText.mockRejectedValue(new Error('plugin device-text not found'))
    native.onDeviceTextChange.mockRejectedValue(new Error('plugin device-text not found'))
    const { initDeviceText, deviceText } = await freshModule()
    await expect(initDeviceText()).resolves.toBeUndefined()
    expect(root.style.getPropertyValue('--device-text-scale')).toBe('')
    expect(deviceText.value).toBeNull()
  })

  it('does not hold launch for a plugin that never answers', async () => {
    vi.useFakeTimers()
    native.getDeviceText.mockReturnValue(new Promise(() => {}))
    const { initDeviceText } = await freshModule()
    let done = false
    const init = initDeviceText().then(() => {
      done = true
    })
    await vi.advanceTimersByTimeAsync(499)
    expect(done).toBe(false)
    await vi.advanceTimersByTimeAsync(1)
    await init
    expect(done).toBe(true)
  })
})
