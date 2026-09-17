import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import type { CollectionPage } from '@nosdesk/core/services/collectionService'
import { mountWithProviders } from '@/test/mountWithProviders'
import CollectionTreeList from '@/components/documentationComponents/CollectionTreeList.vue'

let wrapper: VueWrapper | null = null
let router: Router

const page = (id: number, title: string, extra: Partial<CollectionPage> = {}): CollectionPage =>
  ({
    id,
    uuid: `u${id}`,
    slug: title.toLowerCase(),
    title,
    parent_id: null,
    status: 'published',
    icon: null,
    display_order: id,
    ...extra,
  }) as CollectionPage

// Alpha (with Beta and Gamma under it), then Delta at the root.
const PAGES = [
  page(1, 'Alpha'),
  page(2, 'Beta', { parent_id: 1 }),
  page(3, 'Gamma', { parent_id: 1, status: 'draft' }),
  page(4, 'Delta'),
]

beforeEach(async () => {
  router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  await router.push('/documentation/collections/demo')
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const rows = () => wrapper!.findAll('[role="treeitem"]')
const row = (title: string) => rows().find((r) => r.text().includes(title))!

async function key(el: Element, key: string) {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  await nextTick()
}

function mountTree(props: Record<string, unknown> = {}) {
  wrapper = mountWithProviders(CollectionTreeList, { pages: PAGES, ...props }, {}, [router])
}

describe('CollectionTreeList', () => {
  it('is a tree of linked rows with levels, open parents and badges', async () => {
    mountTree({ overridePageIds: new Set([4]) })
    await nextTick()
    expect(wrapper!.get('[role="tree"]').exists()).toBe(true)
    expect(rows().map((r) => r.attributes('aria-level'))).toEqual(['1', '2', '2', '1'])
    expect(row('Alpha').attributes('aria-expanded')).toBe('true')
    expect(row('Delta').attributes('aria-expanded')).toBeUndefined()
    expect(row('Beta').element.tagName).toBe('A')
    expect(row('Beta').attributes('href')).toBe('/documentation/beta')
    expect(row('Gamma').text()).toContain('docs-collection-tree-item-draft')
    expect(row('Delta').find('[role="img"]').attributes('aria-label')).toBe('docs-collection-tree-item-override-title')
    // No nested controls inside a row.
    expect(wrapper!.findAll('[role="treeitem"] a, [role="treeitem"] button')).toHaveLength(0)
  })

  it('walks with the arrows and collapses and expands from the keyboard', async () => {
    mountTree()
    await nextTick()
    const alpha = row('Alpha').element as HTMLElement
    alpha.focus()
    await key(alpha, 'ArrowDown')
    expect(document.activeElement).toBe(row('Beta').element)
    await key(alpha, 'ArrowLeft')
    expect(row('Alpha').attributes('aria-expanded')).toBe('false')
    expect(rows()).toHaveLength(2)
    await key(alpha, 'ArrowRight')
    expect(row('Alpha').attributes('aria-expanded')).toBe('true')
    expect(rows()).toHaveLength(4)
  })

  // Its own mount: Reka feeds every key, arrows included, into the
  // one-second typeahead buffer.
  it('finds a row by typing its title, icon notwithstanding', async () => {
    mountTree({ pages: PAGES.map((p) => ({ ...p, icon: '🚀' })) })
    await nextTick()
    const alpha = row('Alpha').element as HTMLElement
    alpha.focus()
    await key(alpha, 'd')
    expect(document.activeElement).toBe(row('Delta').element)
  })

  it('navigates on click and Enter without collapsing; the chevron collapses', async () => {
    mountTree()
    await nextTick()
    await row('Alpha').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/documentation/alpha')
    expect(row('Alpha').attributes('aria-expanded')).toBe('true')
    expect(row('Alpha').attributes('aria-selected')).toBe('true')
    expect(row('Delta').attributes('aria-selected')).toBe('false')

    await key(row('Delta').element, 'Enter')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/documentation/delta')
    expect(row('Delta').attributes('aria-selected')).toBe('true')

    await row('Alpha').get('span[aria-hidden]:nth-child(3)').trigger('click')
    expect(row('Alpha').attributes('aria-expanded')).toBe('false')
    expect(router.currentRoute.value.path).toBe('/documentation/delta')
  })

  it('opens parents that arrive after mount and shows the empty state before', async () => {
    mountTree({ pages: [] })
    await nextTick()
    expect(wrapper!.text()).toContain('docs-collection-tree-list-empty')
    await wrapper!.setProps({ pages: PAGES })
    await nextTick()
    expect(rows()).toHaveLength(4)
    expect(row('Alpha').attributes('aria-expanded')).toBe('true')
  })
})
