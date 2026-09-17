import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { ConfigProvider } from 'reka-ui'
import ToastContainer from '@/components/common/ToastContainer.vue'
import { useToastStore } from '@nosdesk/core/stores/toast'

let wrapper: VueWrapper | null = null
let pinia: Pinia
const router = createRouter({
  history: createMemoryHistory(),
  routes: [
    { path: '/', component: { template: '<div />' } },
    { path: '/tickets/:id', component: { template: '<div />' } },
  ],
})

beforeEach(() => {
  vi.useFakeTimers()
  pinia = createPinia()
  setActivePinia(pinia)
  document.body.innerHTML = '<div id="app"></div><div id="overlays"></div>'
  const fluent = createFluentVue({ bundles: [new FluentBundle('en-US')] })
  const Host = defineComponent({ setup: () => () => h(ConfigProvider, {}, () => h(ToastContainer)) })
  wrapper = mount(Host, { attachTo: document.getElementById('app')!, global: { plugins: [fluent, pinia, router] } })
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.useRealTimers()
})

const settle = async () => {
  await nextTick()
  await nextTick()
}

describe('ToastContainer', () => {
  it('renders the queue in a named region with title, description and a dismiss button', async () => {
    const store = useToastStore()
    store.success('Saved', 'Your changes are in.')
    await settle()
    const region = document.querySelector('#overlays [role="region"]')!
    expect(region.getAttribute('aria-label')).toBe('toast-region-label')
    const toast = region.querySelector('li')!
    expect(toast.getAttribute('data-state')).toBe('open')
    // Layout classes land on the list the toasts live in, not the region.
    expect(toast.parentElement?.tagName).toBe('OL')
    expect(toast.parentElement?.classList.contains('flex-col-reverse')).toBe(true)
    expect(toast.textContent).toContain('Saved')
    expect(toast.textContent).toContain('Your changes are in.')
    expect(toast.querySelector('[aria-label="common-toast-dismiss"]')).not.toBeNull()
    // Announced politely (the live region renders a beat after mount):
    // confirmations wait their turn.
    vi.advanceTimersByTime(1_100)
    await settle()
    expect(document.querySelector('[role="alert"]')?.getAttribute('aria-live')).toBe('polite')
  })

  it('announces errors assertively and never times them out', async () => {
    const store = useToastStore()
    store.error('Save failed')
    await settle()
    vi.advanceTimersByTime(1_100)
    await settle()
    expect(document.querySelector('[role="alert"]')?.getAttribute('aria-live')).toBe('assertive')
    vi.advanceTimersByTime(60_000)
    await settle()
    expect(store.toasts).toHaveLength(1)
    expect(document.querySelector('#overlays li')).not.toBeNull()
  })

  it('times a toast out after its duration and drops it from the store', async () => {
    const store = useToastStore()
    store.info('Heads up')
    await settle()
    expect(document.querySelector('#overlays li')).not.toBeNull()
    vi.advanceTimersByTime(5_000)
    await settle()
    expect(document.querySelector('#overlays li')?.getAttribute('data-state') ?? 'gone').not.toBe('open')
    vi.advanceTimersByTime(300)
    await settle()
    expect(store.toasts).toHaveLength(0)
  })

  it('closes from the dismiss button and runs an action before closing', async () => {
    const store = useToastStore()
    const undo = vi.fn()
    store.success('Deleted', undefined, { label: 'Undo', handler: undo })
    await settle()
    const action = document.querySelector('#overlays li button:not([aria-label])') as HTMLButtonElement
    expect(action.textContent?.trim()).toBe('Undo')
    action.click()
    await settle()
    expect(undo).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(300)
    await settle()
    expect(store.toasts).toHaveLength(0)

    store.warning('Careful')
    await settle()
    ;(document.querySelector('#overlays [aria-label="common-toast-dismiss"]') as HTMLButtonElement).click()
    await settle()
    vi.advanceTimersByTime(300)
    await settle()
    expect(store.toasts).toHaveLength(0)
  })

  it('opens the ticket from a notification toast on click and on Enter', async () => {
    const store = useToastStore()
    const push = vi.spyOn(router, 'push')
    store.notification('New comment', undefined, 'ticket', 1, 42, 'Ana')
    await settle()
    const toast = document.querySelector('#overlays li') as HTMLElement
    expect(toast.textContent).toContain('toast-notification-view')
    toast.click()
    expect(push).toHaveBeenCalledWith('/tickets/42')
    await settle()
    expect(store.toasts).toHaveLength(0)

    store.notification('Another', undefined, 'ticket', 1, 7)
    await settle()
    const next = document.querySelector('#overlays li') as HTMLElement
    next.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    expect(push).toHaveBeenLastCalledWith('/tickets/7')
  })
})
