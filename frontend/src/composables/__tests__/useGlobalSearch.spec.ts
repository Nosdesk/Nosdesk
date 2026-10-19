import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const { search } = vi.hoisted(() => ({
  search: vi.fn(async ({ q }: { q: string; types?: string }) => ({
    results: [],
    total: 0,
    query: q,
    took_ms: 1,
  })),
}))
vi.mock('@nosdesk/core/services/searchService', () => ({ searchService: { search } }))

import { useGlobalSearch } from '@/composables/useGlobalSearch'

// The app's callers of the composable: the navbar's search button, the docs
// toolbar's and the palette itself each call it.
const caller = (id: string) =>
  defineComponent({
    setup() {
      const { openSearch } = useGlobalSearch()
      return () => h('button', { id, onClick: () => openSearch() }, id)
    },
  })
const Host = defineComponent({
  setup() {
    return () => h('div', [h(caller('navbar')), h(caller('docs-toolbar')), h(caller('palette'))])
  },
})

const router = createRouter({
  history: createMemoryHistory(),
  routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
})

let wrapper: VueWrapper | null = null
afterEach(() => {
  useGlobalSearch().closeSearch()
  wrapper?.unmount()
  wrapper = null
  search.mockClear()
})

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms))

async function mountCallers() {
  await router.push('/')
  wrapper = mountWithProviders(Host, {}, {}, [router])
  await nextTick()
}

/** Type `text` one character at a time, `gap` ms apart. */
async function type(text: string, gap: number) {
  const { query } = useGlobalSearch()
  for (const ch of text) {
    query.value += ch
    await wait(gap)
  }
  await wait(200)
  await flushPromises()
}

describe('useGlobalSearch', () => {
  it('sends one search per pause in typing, however many components use it', async () => {
    await mountCallers()
    wrapper!.get('#navbar').element.dispatchEvent(new MouseEvent('click'))
    await nextTick()

    await type('vpn', 250)

    const queries = search.mock.calls.map(([params]) => params.q)
    expect(queries).toEqual(['v', 'vp', 'vpn'])
  })

  it('searches once when the scope changes', async () => {
    await mountCallers()
    const { openSearch, applyScope, query } = useGlobalSearch()
    openSearch()
    query.value = 'printer'
    await wait(200)
    await flushPromises()
    search.mockClear()

    applyScope('documentation')
    await flushPromises()

    expect(search).toHaveBeenCalledTimes(1)
    expect(search.mock.calls[0][0]).toMatchObject({ q: 'printer', types: 'documentation' })
  })

  it('looks a `from:` person up once per keystroke', async () => {
    await mountCallers()
    const { openSearch, query } = useGlobalSearch()
    openSearch()
    query.value = 'from:no'
    await flushPromises()
    await wait(200)
    await flushPromises()

    const lookups = search.mock.calls.filter(([params]) => params.types === 'user')
    expect(lookups).toHaveLength(1)
  })
})
