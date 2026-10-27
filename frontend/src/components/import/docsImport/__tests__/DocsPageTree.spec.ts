import { afterEach, describe, expect, it } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { DocsTreePage } from '@nosdesk/core/services/docsImportService'

import { mountWithProviders } from '@/test/mountWithProviders'

import DocsPageTree from '../DocsPageTree.vue'
import { buildTree, toggle, type TreeNode } from '../docsTreeModel'

function page(index: number, title: string, parent: number | null, file: string | null, folder: string | null = null): DocsTreePage {
  return { index, title, path: file ?? folder!, file, folder, parent, order: index, collection: 0 }
}
const tree = buildTree([
  page(0, 'Onboarding', null, 'Onboarding.md', 'Onboarding'),
  page(1, 'Day one', 0, 'Onboarding/Day one.md'),
  page(2, 'Laptop', 0, 'Onboarding/Laptop.md'),
  page(3, 'Network', null, null, 'Network'),
  page(4, 'VPN', 3, 'Network/VPN.md'),
])
const groups = [{ key: 'new', name: 'IT Handbook', kind: 'new' as const, nodes: tree.roots }]

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

function mount(props: Record<string, unknown> = {}) {
  wrapper = mountWithProviders(DocsPageTree, {
    tree,
    groups,
    expanded: new Set(['Onboarding.md']),
    off: new Set(),
    label: 'Pages',
    ...props,
  })
  document.body.appendChild(wrapper.element)
  return wrapper
}

const tree_ = () => wrapper!.findComponent(DocsPageTree)
const items = () => wrapper!.findAll('[role="treeitem"]')
const titles = () => items().map((i) => i.find('[data-test="docs-tree-title"]').text())

describe('DocsPageTree', () => {
  it('is a tree with one tab stop, levels and the collection head', () => {
    mount()
    expect(wrapper!.find('[role="tree"]').attributes('aria-label')).toBe('Pages')
    expect(wrapper!.find('[data-test="docs-tree-head"]').text()).toContain('IT Handbook')
    expect(titles()).toEqual(['Onboarding', 'Day one', 'Laptop', 'Network'])
    expect(items().map((i) => i.attributes('tabindex'))).toEqual(['0', '-1', '-1', '-1'])
    expect(items()[1].attributes('aria-level')).toBe('2')
    expect(items()[0].attributes('aria-expanded')).toBe('true')
    expect(items()[3].attributes('aria-expanded')).toBe('false')
  })

  it('moves with the arrow keys, expands with Right, collapses with Left and ticks with Space', async () => {
    mount()
    await items()[0].trigger('keydown', { key: 'ArrowDown' })
    await flushPromises()
    expect(items()[1].attributes('tabindex')).toBe('0')
    await items()[1].trigger('keydown', { key: 'End' })
    await flushPromises()
    expect(items()[3].attributes('tabindex')).toBe('0')

    await items()[3].trigger('keydown', { key: 'ArrowRight' })
    const expanded = tree_().emitted('update:expanded')!.at(-1)![0] as Set<string>
    expect([...expanded].sort()).toEqual(['Network', 'Onboarding.md'])

    await items()[0].trigger('keydown', { key: 'ArrowLeft' })
    expect([...(tree_().emitted('update:expanded')!.at(-1)![0] as Set<string>)]).toEqual([])

    await items()[2].trigger('keydown', { key: ' ' })
    expect((tree_().emitted('toggle')![0][0] as TreeNode).page.title).toBe('Laptop')
  })

  it('shows a dash on a parent with some pages off, and disables the ticks under an unticked page', () => {
    const off = toggle(tree.byPath.get('Onboarding/Day one.md')!, new Set())
    mount({ off })
    expect(items()[0].attributes('aria-checked')).toBe('mixed')
    expect(items()[1].attributes('aria-checked')).toBe('false')
    wrapper!.unmount()
    mount({ off: new Set(['Onboarding.md']) })
    const ticks = wrapper!.findAll('[data-test="docs-tree-tick"]')
    expect(ticks[0].attributes('disabled')).toBeUndefined()
    expect(ticks[1].attributes('disabled')).toBeDefined()
    expect(items()[1].find('[data-test="docs-tree-title"]').classes()).toContain('line-through')
  })

  it('greys pages a pattern leaves out, with the pattern', () => {
    mount({ patterns: ['Laptop.md'] })
    expect(items()[2].find('[data-test="docs-tree-left-out"]').exists()).toBe(true)
    expect(items()[2].text()).toContain('Laptop.md')
    expect(wrapper!.findAll('[data-test="docs-tree-tick"]')[2].attributes('disabled')).toBeDefined()
  })

  it('shows each outcome on the detail page', () => {
    mount({
      mode: 'detail',
      outcomes: new Map([
        ['Onboarding.md', 'new'],
        ['Onboarding/Day one.md', 'skipped'],
        ['Onboarding/Laptop.md', 'error'],
        ['Network', 'left'],
      ]),
    })
    expect(wrapper!.findAll('[data-test="docs-tree-tick"]')).toHaveLength(0)
    expect(wrapper!.findAll('[data-test="docs-tree-outcome"]').map((p) => p.text())).toEqual([
      'docs-import-outcome-new',
      'docs-import-outcome-skipped',
      'docs-import-outcome-error',
      'docs-import-outcome-left',
    ])
  })

  it('shows what each page is and how many images it has on Pages', () => {
    const kinds = buildTree([
      { ...page(0, 'Hardware', null, 'Hardware.md', 'Hardware'), kind: 'page', images: 3 },
      { ...page(1, 'Vendors', 0, null, 'Hardware/Vendors'), kind: 'database' },
      { ...page(2, 'Dell', 1, 'Hardware/Vendors/Dell.md'), kind: 'database_row', images: 1 },
      { ...page(3, 'Network', null, null, 'Network'), kind: 'section' },
    ])
    mount({
      tree: kinds,
      groups: [{ key: 'new', name: 'IT Handbook', kind: 'new' as const, nodes: kinds.roots }],
      expanded: new Set(['Hardware.md', 'Hardware/Vendors']),
    })
    const row = (i: number) => items()[i]
    expect(row(1).find('[data-test="docs-tree-database"]').text()).toBe('docs-import-tree-database')
    expect(row(1).find('[data-test="docs-tree-no-content"]').exists()).toBe(false)
    expect(row(3).find('[data-test="docs-tree-no-content"]').exists()).toBe(true)
    expect(wrapper!.findAll('[data-test="docs-tree-database"]')).toHaveLength(1)
    expect(row(0).find('[data-test="docs-tree-images"]').text()).toBe('docs-import-tree-image-count')
    expect(row(2).find('[data-test="docs-tree-images"]').exists()).toBe(true)
    expect(row(3).find('[data-test="docs-tree-images"]').exists()).toBe(false)
    // A tree read before kinds were kept: a page with no file has no content.
    wrapper!.unmount()
    mount()
    expect(items()[3].find('[data-test="docs-tree-no-content"]').exists()).toBe(true)
  })

  it('links each page the import wrote on the detail page, and shows Trash and Kept after removal', async () => {
    const view = { template: '<div />' }
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/', component: view },
        { path: '/documentation/:path', name: 'documentation-page', component: view },
      ],
    })
    await router.push('/')
    wrapper = mountWithProviders(
      DocsPageTree,
      {
        tree,
        groups,
        expanded: new Set(['Onboarding.md']),
        label: 'Pages',
        mode: 'detail',
        outcomes: new Map([
          ['Onboarding.md', 'kept'],
          ['Onboarding/Day one.md', 'trash'],
          ['Onboarding/Laptop.md', 'skipped'],
          ['Network', 'left'],
        ]),
        links: new Map([
          ['Onboarding.md', 41],
          ['Onboarding/Day one.md', 42],
        ]),
      },
      {},
      [router],
    )
    await flushPromises()
    const links = wrapper.findAll('[data-test="docs-tree-link"]')
    expect(links.map((l) => [l.text(), l.attributes('href')])).toEqual([
      ['Onboarding', '/documentation/41'],
      ['Day one', '/documentation/42'],
    ])
    expect(links[0].attributes('tabindex')).toBe('-1')
    expect(wrapper.findAll('[data-test="docs-tree-title"]').map((t) => t.text())).toEqual(['Laptop', 'Network'])
    expect(wrapper.findAll('[data-test="docs-tree-outcome"]').map((p) => p.text())).toEqual([
      'docs-import-outcome-kept',
      'docs-import-outcome-trash',
      'docs-import-outcome-skipped',
      'docs-import-outcome-left',
    ])
  })
})
