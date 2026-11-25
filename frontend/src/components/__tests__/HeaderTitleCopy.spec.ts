import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import HeaderTitleCopy from '@/components/HeaderTitleCopy.vue'
import PageTitleSlot from '@/components/PageTitleSlot.vue'

// The bar's one-line copy follows the page title's slot: shown once the
// slot has gone up past the screen, transitioned only after the first
// reading, and a tap brings the title back.

let callback: IntersectionObserverCallback | null = null
class StubIntersectionObserver {
  constructor(cb: IntersectionObserverCallback) {
    callback = cb
  }
  observe() {}
  unobserve() {}
  disconnect() {}
}
function read(isIntersecting: boolean, top: number) {
  const entry = { isIntersecting, boundingClientRect: { top, height: 120 } } as unknown as IntersectionObserverEntry
  callback!([entry], {} as IntersectionObserver)
}

const Page = defineComponent({
  setup() {
    return () => h('div', [h(HeaderTitleCopy, { text: 'Printer jam on level 3', fallback: 'Untitled' }), h(PageTitleSlot)])
  },
})

let wrapper: VueWrapper | null = null
beforeEach(() => {
  vi.stubGlobal('IntersectionObserver', StubIntersectionObserver)
  vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => setTimeout(() => fn(0), 0) as unknown as number)
  vi.stubGlobal('cancelAnimationFrame', (id: number) => clearTimeout(id))
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  callback = null
  vi.unstubAllGlobals()
})

const frame = () => new Promise((resolve) => setTimeout(resolve, 0))
const copy = () => wrapper!.get('[data-header-title-copy]')

describe('HeaderTitleCopy', () => {
  it('is shown once the title has gone, and transitions only after the first reading', async () => {
    wrapper = mount(Page, { attachTo: document.body })
    expect(copy().classes()).not.toContain('is-shown')
    expect(copy().classes()).not.toContain('is-settled')
    expect(copy().attributes('aria-hidden')).toBe('true')

    // A page restored mid-scroll: shown at once, not yet transitioned.
    read(false, -400)
    await nextTick()
    expect(copy().classes()).toContain('is-shown')
    expect(copy().classes()).not.toContain('is-settled')

    await frame()
    await nextTick()
    expect(copy().classes()).toContain('is-settled')

    read(true, 80)
    await nextTick()
    expect(copy().classes()).not.toContain('is-shown')
    expect(copy().classes()).toContain('is-settled')
  })

  it('shows the fallback, quieter, for an empty title', () => {
    wrapper = mount(
      defineComponent({ setup: () => () => h(HeaderTitleCopy, { text: '', fallback: 'Untitled Ticket' }) }),
    )
    expect(copy().text()).toBe('Untitled Ticket')
    expect(copy().classes()).toContain('text-tertiary')
  })

  it('scrolls the title back into view on a tap', async () => {
    const scrollIntoView = vi.fn()
    const original = Element.prototype.scrollIntoView
    Element.prototype.scrollIntoView = scrollIntoView
    try {
      wrapper = mount(Page, { attachTo: document.body })
      read(false, -400)
      await nextTick()
      await copy().trigger('click')
      expect(scrollIntoView).toHaveBeenCalledTimes(1)
      expect(scrollIntoView.mock.contexts[0]).toBe(wrapper.get('[data-page-title]').element)
      expect(scrollIntoView.mock.calls[0]![0]).toMatchObject({ block: 'end' })
    } finally {
      Element.prototype.scrollIntoView = original
    }
  })
})
