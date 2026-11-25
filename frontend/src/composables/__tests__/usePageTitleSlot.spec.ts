import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick, ref } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import {
  pageTitleAway,
  pageTitleSettled,
  pageTitleTarget,
  usePageTitleSlot,
} from '@/composables/usePageTitleSlot'

// jsdom has no IntersectionObserver: a stub the test reads out by hand.
type Reading = { isIntersecting: boolean; top: number; height: number }
type ObserverRecord = { callback: IntersectionObserverCallback; disconnected: boolean; calls: string[] }
let observers: ObserverRecord[] = []
class StubIntersectionObserver {
  record: ObserverRecord
  constructor(callback: IntersectionObserverCallback) {
    this.record = { callback, disconnected: false, calls: [] }
    observers.push(this.record)
  }
  observe() {
    this.record.calls.push('observe')
  }
  unobserve() {
    this.record.calls.push('unobserve')
  }
  disconnect() {
    this.record.disconnected = true
  }
}
// ResizeObserver: the callback, for the test to fire by hand.
let resized: (() => void)[] = []
class StubResizeObserver {
  constructor(callback: ResizeObserverCallback) {
    resized.push(() => callback([], {} as ResizeObserver))
  }
  observe() {}
  unobserve() {}
  disconnect() {}
}
const entryOf = (reading: Reading) =>
  ({
    isIntersecting: reading.isIntersecting,
    boundingClientRect: { top: reading.top, height: reading.height },
  }) as unknown as IntersectionObserverEntry
function read(index: number, ...readings: Reading[]) {
  observers[index]!.callback(readings.map(entryOf), {} as IntersectionObserver)
}

const Slot = defineComponent({
  setup() {
    const el = ref<HTMLElement | null>(null)
    usePageTitleSlot(el)
    return () => h('div', { ref: el, 'data-page-title': '' })
  },
})

let wrappers: VueWrapper[] = []
beforeEach(() => {
  observers = []
  resized = []
  vi.stubGlobal('IntersectionObserver', StubIntersectionObserver)
  vi.stubGlobal('ResizeObserver', StubResizeObserver)
  vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => setTimeout(() => fn(0), 0) as unknown as number)
  vi.stubGlobal('cancelAnimationFrame', (id: number) => clearTimeout(id))
})
afterEach(() => {
  for (const w of wrappers) w.unmount()
  wrappers = []
  vi.unstubAllGlobals()
})

const frame = () => new Promise((resolve) => setTimeout(resolve, 0))

describe('usePageTitleSlot', () => {
  it('is the target while mounted and clears when it unmounts', () => {
    expect(pageTitleTarget.value).toBe(null)
    const wrapper = mount(Slot)
    expect(pageTitleTarget.value).toBe(wrapper.element)
    wrapper.unmount()
    expect(pageTitleTarget.value).toBe(null)
    expect(observers[0]!.disconnected).toBe(true)
  })

  it('is away only once the whole slot has gone up past the top', async () => {
    wrappers.push(mount(Slot))
    read(0, { isIntersecting: true, top: 80, height: 120 })
    expect(pageTitleAway.value).toBe(false)
    read(0, { isIntersecting: false, top: -130, height: 120 })
    expect(pageTitleAway.value).toBe(true)
    // Below the screen (a page scrolled up past its start) is not away.
    read(0, { isIntersecting: false, top: 5000, height: 120 })
    expect(pageTitleAway.value).toBe(false)
    // No box: hidden from `sm` up.
    read(0, { isIntersecting: false, top: 0, height: 0 })
    expect(pageTitleAway.value).toBe(false)
  })

  it('settles a frame after the first reading, so a restored scroll shows the copy at once', async () => {
    wrappers.push(mount(Slot))
    expect(pageTitleSettled.value).toBe(false)
    read(0, { isIntersecting: false, top: -400, height: 120 })
    expect(pageTitleAway.value).toBe(true)
    expect(pageTitleSettled.value).toBe(false)
    await frame()
    expect(pageTitleSettled.value).toBe(true)
  })

  it('leaves the next view’s slot alone when the last one unmounts after it mounted', async () => {
    const first = mount(Slot)
    read(0, { isIntersecting: false, top: -400, height: 120 })
    const second = mount(Slot)
    wrappers.push(second)
    expect(pageTitleTarget.value).toBe(second.element)
    expect(pageTitleAway.value).toBe(false)
    read(1, { isIntersecting: false, top: -400, height: 120 })
    // The first slot's observer no longer speaks for the bar.
    read(0, { isIntersecting: true, top: 80, height: 120 })
    expect(pageTitleAway.value).toBe(true)
    first.unmount()
    await nextTick()
    expect(pageTitleTarget.value).toBe(second.element)
    expect(pageTitleAway.value).toBe(true)
  })

  it('reads the newest of several queued entries', () => {
    wrappers.push(mount(Slot))
    read(0, { isIntersecting: true, top: 80, height: 120 }, { isIntersecting: false, top: -400, height: 120 })
    expect(pageTitleAway.value).toBe(true)
    read(0, { isIntersecting: false, top: -400, height: 120 }, { isIntersecting: true, top: 80, height: 120 })
    expect(pageTitleAway.value).toBe(false)
  })

  it('asks for a fresh reading when the slot changes size (the title arriving)', () => {
    wrappers.push(mount(Slot))
    // Restored above the screen while still empty: not away.
    read(0, { isIntersecting: false, top: -400, height: 0 })
    expect(pageTitleAway.value).toBe(false)
    observers[0]!.calls.length = 0
    resized[0]!()
    expect(observers[0]!.calls).toEqual(['unobserve', 'observe'])
    // The reading a re-observe brings, now with the title in it.
    read(0, { isIntersecting: false, top: -400, height: 120 })
    expect(pageTitleAway.value).toBe(true)
  })
})
