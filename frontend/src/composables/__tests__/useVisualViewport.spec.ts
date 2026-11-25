import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, ref } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import { useVisualViewport } from '@/composables/useVisualViewport'

/** A visual viewport the test moves by hand. */
class FakeViewport extends EventTarget {
  height = 800
  offsetTop = 0
  scale = 1
  set(next: Partial<Pick<FakeViewport, 'height' | 'offsetTop' | 'scale'>>) {
    Object.assign(this, next)
    this.dispatchEvent(new Event('resize'))
  }
}

let vv: FakeViewport
let wrapper: VueWrapper | null = null
let keyboard: { value: number }

beforeEach(() => {
  vv = new FakeViewport()
  vi.stubGlobal('visualViewport', vv)
  vi.stubGlobal('innerHeight', 800)
  // The frame runs at once; handing back 0 means no frame is pending.
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
    cb(0)
    return 0
  })
  wrapper = mount(
    defineComponent({
      setup() {
        keyboard = useVisualViewport(ref(true)).keyboardHeight
        return () => null
      },
    }),
  )
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.unstubAllGlobals()
})

const published = () => document.documentElement.style.getPropertyValue('--keyboard-height')

describe('useVisualViewport', () => {
  it('publishes the keyboard as the layout height the visual viewport lost', () => {
    vv.set({ height: 500 })
    expect(keyboard.value).toBe(300)
    expect(published()).toBe('300px')
    vv.set({ height: 800 })
    expect(keyboard.value).toBe(0)
  })

  // Pinch zoom shrinks the visual viewport and pans it, which reads exactly
  // like a keyboard opening. A docked bar would jump up the screen.
  it('does not read pinch zoom as a keyboard', () => {
    vv.set({ scale: 2, height: 400, offsetTop: 0 })
    expect(keyboard.value).toBe(0)
    vv.set({ scale: 2, height: 400, offsetTop: 120 })
    expect(keyboard.value).toBe(0)
  })

  it('holds the keyboard it had while zoomed, and measures again at scale 1', () => {
    vv.set({ height: 500 })
    vv.set({ scale: 1.5, height: 333, offsetTop: 200 })
    expect(keyboard.value).toBe(300)
    vv.set({ scale: 1, height: 800, offsetTop: 0 })
    expect(keyboard.value).toBe(0)
  })
})
