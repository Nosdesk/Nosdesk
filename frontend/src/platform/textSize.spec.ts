import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { shallowRef } from 'vue'
import { composeScale, nearestStop } from '@/utils/textScale'

const device = shallowRef<{ scale: number; bold: boolean } | null>(null)
let follow = true
let tauri = false

vi.mock('./deviceText', () => ({
  get FOLLOW_DEVICE_TEXT() {
    return follow
  },
  deviceText: device,
}))
vi.mock('./runtime', () => ({ isTauriRuntime: () => tauri }))

const root = document.documentElement
const userScale = () => root.style.getPropertyValue('--user-text-scale')

async function load() {
  vi.resetModules()
  return import('./textSize')
}

beforeEach(() => {
  localStorage.clear()
  root.style.removeProperty('--user-text-scale')
  device.value = null
  follow = true
  tauri = false
})
afterEach(() => vi.restoreAllMocks())

describe('composeScale and nearestStop', () => {
  it('takes the device while following and the own stop otherwise, held to 1 to 2', () => {
    expect(composeScale(true, 1.1176, 150)).toBe(1.1176)
    expect(composeScale(true, 0.82, 150)).toBe(1)
    expect(composeScale(false, 1.1176, 150)).toBe(1.5)
    expect(composeScale(true, 3, 100)).toBe(2)
  })

  it('finds the stop nearest a percentage', () => {
    expect(nearestStop(100)).toBe(0)
    expect(nearestStop(112)).toBe(1)
    expect(nearestStop(130)).toBe(2)
    expect(nearestStop(240)).toBe(5)
  })
})

describe('text size in a browser', () => {
  it('writes nothing at 100% and the own size otherwise, kept across loads', async () => {
    const size = await load()
    size.initTextSize()
    expect(userScale()).toBe('')
    expect(size.canFollowDevice()).toBe(false)

    size.chooseTextSize(150)
    expect(userScale()).toBe('1.5')
    expect(size.textSizePercent.value).toBe(150)

    root.style.removeProperty('--user-text-scale')
    const again = await load()
    again.initTextSize()
    expect(userScale()).toBe('1.5')

    again.chooseTextSize(100)
    expect(userScale()).toBe('')
  })

  it('ignores a size out of range and storage that throws', async () => {
    localStorage.setItem('textSize', JSON.stringify({ follow: false, percent: 400 }))
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    const size = await load()
    size.initTextSize()
    expect(userScale()).toBe('')
    size.chooseTextSize(125)
    expect(userScale()).toBe('1.25')
  })
})

describe('text size in the app', () => {
  beforeEach(() => {
    tauri = true
    device.value = { scale: 1.1176, bold: true }
  })

  it('follows the device by default and writes nothing', async () => {
    const size = await load()
    size.initTextSize()
    expect(size.followsDevice.value).toBe(true)
    expect(size.textSizePercent.value).toBe(112)
    expect(userScale()).toBe('')
  })

  it('takes the size over when one is chosen, and the switch returns to the device', async () => {
    const size = await load()
    size.initTextSize()
    size.chooseTextSize(150)
    expect(size.followsDevice.value).toBe(false)
    expect(userScale()).toBe('1.5')

    size.setFollowDeviceText(true)
    expect(size.textSizePercent.value).toBe(112)
    expect(userScale()).toBe('')
  })

  it('keeps the device size exactly as its own when following is switched off', async () => {
    const size = await load()
    size.initTextSize()
    size.setFollowDeviceText(false)
    expect(size.textSize.percent).toBe(111.76)
    expect(size.textSizePercent.value).toBe(112)
    expect(userScale()).toBe('1.1176')
  })

  it('goes back to following when the saved size cannot be applied', async () => {
    localStorage.setItem('textSize', JSON.stringify({ follow: false, percent: 'large' }))
    const size = await load()
    size.initTextSize()
    expect(size.followsDevice.value).toBe(true)
    expect(userScale()).toBe('')
  })

  it('an own size of 100% still overrides a larger device', async () => {
    const size = await load()
    size.initTextSize()
    size.chooseTextSize(100)
    expect(userScale()).toBe('1')
  })

  it('acts as a browser until the app applies device text', async () => {
    follow = false
    const size = await load()
    size.initTextSize()
    expect(size.canFollowDevice()).toBe(false)
    expect(size.textSizePercent.value).toBe(100)
  })

  it('names the platform from the webview', async () => {
    const size = await load()
    expect(size.textSizePlatform()).toBe('ios')
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue('Mozilla/5.0 (Linux; Android 15)')
    expect(size.textSizePlatform()).toBe('android')
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)')
    Object.defineProperty(navigator, 'maxTouchPoints', { value: 5, configurable: true })
    expect(size.textSizePlatform()).toBe('ipad')
  })
})
