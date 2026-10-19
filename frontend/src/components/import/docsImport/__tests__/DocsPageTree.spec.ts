import { afterEach, describe, expect, it } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
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
})
