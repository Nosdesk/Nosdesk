import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

vi.mock('@nosdesk/core/services/searchService', () => ({
  searchService: {
    search: async ({ q }: { q: string }) => ({
      results: [
        { id: 'ticket-1', entity_type: 'ticket', entity_id: 1, title: `Ticket ${q}`, preview: '', url: '/tickets/1', score: 1 },
        { id: 'ticket-2', entity_type: 'ticket', entity_id: 2, title: `Other ${q}`, preview: '', url: '/tickets/2', score: 0.9 },
        { id: 'user-3', entity_type: 'user', entity_id: 3, title: 'Noah', preview: '', url: '/users/3', score: 0.5 },
      ],
      total: 3,
      query: q,
      took_ms: 1,
    }),
  },
}))

import GlobalSearchModal from '@/components/GlobalSearch/GlobalSearchModal.vue'
import { useGlobalSearch } from '@/composables/useGlobalSearch'

// The palette next to an opener button and a bystander, as in the app.
const Host = defineComponent({
  setup() {
    const { openSearch } = useGlobalSearch()
    return () =>
      h('div', [
        h('button', { id: 'opener', onClick: () => openSearch() }, 'open'),
        h('button', { id: 'bystander' }, 'other'),
        h(GlobalSearchModal),
      ])
  },
})

// One router for the file: the composable's window keydown listener is
// registered once for the app's lifetime and closes over the first
// router it saw.
const router = createRouter({
  history: createMemoryHistory(),
  routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
})

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

const settle = (ms = 0) => new Promise((r) => setTimeout(r, ms))
const input = () => document.body.querySelector<HTMLInputElement>('input[role="combobox"]')!
const activeText = () => document.getElementById(input().getAttribute('aria-activedescendant') ?? '')?.textContent?.trim()
async function key(key: string) {
  window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  await nextTick()
}

async function open() {
  await router.push('/')
  wrapper = mountWithProviders(Host, {}, {}, [router])
  await nextTick()
  const opener = wrapper.get('#opener').element as HTMLElement
  opener.focus()
  opener.click()
  await nextTick()
  await nextTick()
  await settle()
}

describe('GlobalSearchModal', () => {
  it('is a labelled modal dialog that hides the page and takes focus into a combobox', async () => {
    await open()
    const dialog = document.body.querySelector('[role="dialog"]')!
    expect(dialog.getAttribute('aria-modal')).toBe('true')
    expect(document.getElementById(dialog.getAttribute('aria-labelledby')!)?.textContent).toBe('search-global-aria-label')
    expect(wrapper!.element.closest('[aria-hidden="true"]') ?? wrapper!.get('#bystander').element.closest('[aria-hidden="true"]')).not.toBeNull()
    expect(document.activeElement).toBe(input())
    expect(input().getAttribute('aria-expanded')).toBe('true')
    // The scope rows are the first list, with the first one active.
    expect(document.body.querySelector('[role="listbox"]')?.id).toBe(input().getAttribute('aria-controls'))
    expect(activeText()).toContain('search-global-scope-row')
    const first = input().getAttribute('aria-activedescendant')
    await key('ArrowDown')
    expect(input().getAttribute('aria-activedescendant')).not.toBe(first)
  })

  it('lists results as grouped options the arrows point at, and Enter opens one', async () => {
    await open()
    input().value = 'hello'
    input().dispatchEvent(new Event('input', { bubbles: true }))
    await flushPromises()
    await settle(200)
    await flushPromises()
    const options = Array.from(document.body.querySelectorAll('[role="listbox"] [role="option"]'))
    expect(options.map((o) => o.getAttribute('aria-selected'))).toEqual(['true', 'false', 'false'])
    const groups = Array.from(document.body.querySelectorAll('[role="listbox"] [role="group"]'))
    // Group names fall back to the built-in English labels without a catalogue.
    expect(groups.map((g) => g.getAttribute('aria-label'))).toEqual(['Tickets', 'Users'])
    // No control other than options inside the listbox.
    expect(document.body.querySelectorAll('[role="listbox"] button:not([role="option"])')).toHaveLength(0)
    expect(activeText()).toContain('Ticket hello')
    await key('ArrowDown')
    expect(activeText()).toContain('Other hello')
    await key('Enter')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/tickets/2')
    await settle()
    expect(document.body.querySelector('[role="dialog"]')).toBeNull()
  })

  it('closes on Escape and hands focus back to the opener', async () => {
    await open()
    await key('Escape')
    await settle()
    expect(document.body.querySelector('[role="dialog"]')).toBeNull()
    // Reka restores focus a macrotask after unmount.
    await settle()
    expect(document.activeElement).toBe(wrapper!.get('#opener').element)
    expect(wrapper!.get('#bystander').element.closest('[aria-hidden="true"]')).toBeNull()
  })
})
