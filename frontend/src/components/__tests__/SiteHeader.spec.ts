import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick, ref } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { VueWrapper } from '@vue/test-utils'

// On a phone a detail view's title leads the page: SiteHeader teleports its
// one editable title into the view's PageTitleSlot and keeps a one-line,
// aria-hidden copy in the bar. Everywhere else the title stays in the bar.

vi.mock('@/components/NotificationBell.vue', () => ({ default: { template: '<span />' } }))
vi.mock('@/components/UserDropdownMenu.vue', () => ({ default: { template: '<span />' } }))
vi.mock('@/components/UserAvatar.vue', () => ({ default: { template: '<span />' } }))
vi.mock('@/components/DocumentIconSelector.vue', () => ({
  default: { template: '<span data-icon-selector />' },
}))

import SiteHeader from '@/components/SiteHeader.vue'
import PageTitleSlot from '@/components/PageTitleSlot.vue'
import { mountWithProviders } from '@/test/mountWithProviders'

const TITLE = 'Hot water system leaking at 14 Banksia St'

class StubIntersectionObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

function setWidth(width: number) {
  Object.defineProperty(window, 'innerWidth', { value: width, configurable: true, writable: true })
}

const withSlot = ref(true)
const Page = defineComponent({
  inheritAttrs: false,
  setup(_, { attrs }) {
    return () =>
      h('div', [
        h(SiteHeader, { ticket: null, device: null, document: null, ...attrs }),
        h('main', [withSlot.value ? h(PageTitleSlot) : null]),
      ])
  },
})

function router() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/', component: { template: '<div />' } }],
  })
}

let wrapper: VueWrapper | null = null
async function mountPage(props: Record<string, unknown>) {
  wrapper = mountWithProviders(Page, props, {}, [router()])
  await nextTick()
  await nextTick()
  return wrapper
}

beforeEach(() => {
  withSlot.value = true
  vi.stubGlobal('IntersectionObserver', StubIntersectionObserver)
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.unstubAllGlobals()
  setWidth(1024)
})

const slot = () => wrapper!.get('[data-page-title]')
const header = () => wrapper!.get('header')
const headings = () => wrapper!.findAll('h1')
const copy = () => wrapper!.find('[data-header-title-copy]')

describe('SiteHeader title on a phone', () => {
  it('puts the ticket title in the page slot as the only h1, with an aria-hidden copy in the bar', async () => {
    setWidth(402)
    await mountPage({ ticket: { id: 7, number: 1482, title: TITLE } })
    expect(headings()).toHaveLength(1)
    expect(slot().find('h1').exists()).toBe(true)
    expect(slot().get('h1').text()).toBe(TITLE)
    expect(header().find('h1').exists()).toBe(false)
    // The ID stays in the bar.
    expect(header().text()).toContain('1482')
    expect(slot().text()).not.toContain('1482')
    // The copy is presentation only.
    expect(copy().exists()).toBe(true)
    expect(copy().attributes('aria-hidden')).toBe('true')
    expect(copy().attributes('tabindex')).toBeUndefined()
    expect(copy().text()).toBe(TITLE)
  })

  it('keeps the title in the bar from `sm` up', async () => {
    setWidth(1024)
    await mountPage({ ticket: { id: 7, number: 1482, title: TITLE } })
    expect(slot().text()).toBe('')
    expect(header().text()).toContain(TITLE)
    expect(header().findAll('[tabindex="0"]').filter((n) => n.text() === TITLE)).toHaveLength(1)
    expect(copy().exists()).toBe(false)
  })

  it('keeps the title in the bar on a phone when the view has no slot, and moves it when one appears', async () => {
    setWidth(402)
    withSlot.value = false
    await mountPage({ ticket: { id: 7, number: 1482, title: TITLE } })
    expect(wrapper!.find('[data-page-title]').exists()).toBe(false)
    expect(header().text()).toContain(TITLE)
    expect(copy().exists()).toBe(false)

    withSlot.value = true
    await nextTick()
    await nextTick()
    expect(slot().get('h1').text()).toBe(TITLE)
    expect(wrapper!.findAll('[tabindex="0"]').filter((n) => n.text() === TITLE)).toHaveLength(1)

    withSlot.value = false
    await nextTick()
    await nextTick()
    expect(headings()).toHaveLength(0)
    expect(wrapper!.findAll('[tabindex="0"]').filter((n) => n.text() === TITLE)).toHaveLength(1)
    expect(header().text()).toContain(TITLE)
  })

  it('shows the placeholder on the page for an empty title, and "Untitled" only in the copy', async () => {
    setWidth(402)
    await mountPage({ ticket: { id: 7, number: 1482, title: '' } })
    expect(slot().get('h1').text()).toBe('ui-site-header-ticket-title-placeholder')
    // Heard as untitled, not as the prompt to type one.
    expect(slot().get('h1').attributes('aria-label')).toBe('ui-site-header-untitled-ticket')
    expect(copy().text()).toBe('ui-site-header-untitled-ticket')
  })

  it('puts a read-only asset name in the slot as a plain h1', async () => {
    setWidth(402)
    await mountPage({ device: { id: 31, name: 'Rheem Stellar 315L' }, deviceTitleEditable: false })
    expect(headings()).toHaveLength(1)
    const heading = slot().get('h1')
    expect(heading.text()).toBe('Rheem Stellar 315L')
    expect(heading.attributes('tabindex')).toBeUndefined()
    expect(copy().text()).toBe('Rheem Stellar 315L')
  })

  it('puts a document title in the slot and keeps the icon selector in the bar', async () => {
    setWidth(402)
    await mountPage({ document: { id: 'doc-1', title: 'Replacing a relief valve', icon: 'wrench' } })
    expect(headings()).toHaveLength(1)
    expect(slot().get('h1').text()).toBe('Replacing a relief valve')
    expect(header().find('[data-icon-selector]').exists()).toBe(true)
  })

  it('keeps exactly one focusable title as the window crosses `sm` and back', async () => {
    const focusable = () => wrapper!.findAll('[tabindex="0"]').filter((n) => n.text() === TITLE)
    const resizeTo = async (width: number) => {
      setWidth(width)
      window.dispatchEvent(new Event('resize'))
      // useMobileDetection debounces resizes by 150ms.
      await new Promise((resolve) => setTimeout(resolve, 200))
      await nextTick()
      await nextTick()
    }
    setWidth(402)
    await mountPage({ ticket: { id: 7, number: 1482, title: TITLE } })
    expect(focusable()).toHaveLength(1)
    expect(slot().find('h1').exists()).toBe(true)

    await resizeTo(1024)
    expect(focusable()).toHaveLength(1)
    expect(slot().text()).toBe('')
    expect(header().text()).toContain(TITLE)
    expect(copy().exists()).toBe(false)

    await resizeTo(402)
    expect(focusable()).toHaveLength(1)
    expect(slot().get('h1').text()).toBe(TITLE)
    expect(copy().exists()).toBe(true)
  })

  it('drops an edit in progress when the subject changes, so it never saves to the next one', async () => {
    setWidth(402)
    const saves: string[] = []
    await mountPage({
      document: { id: 'doc-a', title: 'Payroll runbook', icon: 'book' },
      onUpdateDocumentTitle: (title: string) => saves.push(title),
    })
    ;(slot().get('h1').element as HTMLElement).focus()
    await nextTick()
    await nextTick()
    await nextTick()
    const field = slot().get('textarea').element as HTMLTextAreaElement
    expect(field.hidden).toBe(false)
    field.value = 'Payroll runbook, edited'
    field.dispatchEvent(new Event('input', { bubbles: true }))

    await wrapper!.setProps({ document: { id: 'doc-b', title: 'Leave policy', icon: 'book' } })
    await nextTick()
    await nextTick()
    expect(slot().get('h1').text()).toBe('Leave policy')
    expect((slot().get('textarea').element as HTMLTextAreaElement).hidden).toBe(true)
    expect(saves).toEqual([])
  })
})
