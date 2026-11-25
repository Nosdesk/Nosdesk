import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  DEFAULT_BODY_PX,
  MAX_DEVICE_TEXT_SCALE,
  followDeviceTextSize,
  scaleForBodySize,
} from './textSize'

const root = document.documentElement
const scale = () => root.style.getPropertyValue('--device-text-scale')

function setVisibility(state: DocumentVisibilityState) {
  Object.defineProperty(document, 'visibilityState', { value: state, configurable: true })
  document.dispatchEvent(new Event('visibilitychange'))
}

let stop: () => void = () => {}

beforeEach(() => {
  ;(globalThis as { isTauri?: boolean }).isTauri = true
})

afterEach(() => {
  stop()
  stop = () => {}
  delete (globalThis as { isTauri?: boolean }).isTauri
  root.style.removeProperty('--device-text-scale')
  vi.restoreAllMocks()
})

describe('scaleForBodySize', () => {
  it('is 1 at the default text size', () => {
    expect(scaleForBodySize(DEFAULT_BODY_PX)).toBe(1)
  })

  it('follows a larger size: xLarge (19px) and xxLarge (21px)', () => {
    expect(scaleForBodySize(19)).toBe(1.118)
    expect(scaleForBodySize(21)).toBe(1.235)
  })

  it('never shrinks below 1 for the smaller sizes', () => {
    expect(scaleForBodySize(14)).toBe(1)
  })

  it('stops at the cap for the accessibility sizes', () => {
    expect(scaleForBodySize(23)).toBe(MAX_DEVICE_TEXT_SCALE)
    expect(scaleForBodySize(53)).toBe(MAX_DEVICE_TEXT_SCALE)
  })

  it('is 1 when nothing could be measured', () => {
    expect(scaleForBodySize(null)).toBe(1)
    expect(scaleForBodySize(Number.NaN)).toBe(1)
    expect(scaleForBodySize(0)).toBe(1)
  })
})

describe('followDeviceTextSize', () => {
  it('does nothing in a browser, even on an iPhone', () => {
    delete (globalThis as { isTauri?: boolean }).isTauri
    const measure = vi.fn(() => 19)
    stop = followDeviceTextSize('ios', { measure })
    expect(measure).not.toHaveBeenCalled()
    expect(scale()).toBe('')
  })

  it('does nothing in the Android or desktop app', () => {
    const measure = vi.fn(() => 19)
    for (const os of ['android', 'macos', 'windows', null]) {
      followDeviceTextSize(os, { measure })()
    }
    expect(measure).not.toHaveBeenCalled()
    expect(scale()).toBe('')
  })

  it('sets the root scale from the measured size in the iOS app', () => {
    stop = followDeviceTextSize('ios', { measure: () => 19 })
    expect(scale()).toBe('1.118')
  })

  it('measures again when the app comes back from Settings, not when it leaves', () => {
    let px = 17
    const measure = vi.fn(() => px)
    stop = followDeviceTextSize('ios', { measure })
    expect(scale()).toBe('1')

    px = 23
    setVisibility('hidden')
    expect(scale()).toBe('1')
    setVisibility('visible')
    expect(scale()).toBe(String(MAX_DEVICE_TEXT_SCALE))
  })

  it('measures again on resize', () => {
    let px = 17
    stop = followDeviceTextSize('ios', { measure: () => px })
    px = 19
    window.dispatchEvent(new Event('resize'))
    expect(scale()).toBe('1.118')
  })

  it('reads the probe styled with the system body font, and stop removes it and the scale', () => {
    const real = window.getComputedStyle
    vi.spyOn(window, 'getComputedStyle').mockImplementation((el, pseudo) => {
      const style = real(el, pseudo)
      if (el.hasAttribute('data-text-size-probe')) {
        return { ...style, fontSize: '21px' } as CSSStyleDeclaration
      }
      return style
    })
    stop = followDeviceTextSize('ios')
    expect(scale()).toBe('1.235')
    expect(root.querySelector('[data-text-size-probe]')).not.toBeNull()

    stop()
    stop = () => {}
    expect(scale()).toBe('')
    expect(root.querySelector('[data-text-size-probe]')).toBeNull()
  })
})
