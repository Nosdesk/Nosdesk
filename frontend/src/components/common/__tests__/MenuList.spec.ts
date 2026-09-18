import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { h } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'

const settle = () => new Promise((r) => setTimeout(r, 20))

let wrapper: VueWrapper | null = null

beforeEach(() => {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes('min-width'),
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    onchange: null,
    dispatchEvent: () => false,
  }))
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

const ITEMS: MenuItem[] = [
  { id: 'sort', label: 'Sort by', heading: true },
  { id: 'created', label: 'Created', checked: true },
  { id: 'updated', label: 'Updated', checked: false },
  { id: 'delete', label: 'Delete', divider: true, danger: true },
]

describe('MenuList', () => {
  it('renders checked rows as radio items, dividers as separators, and emits the picked id', async () => {
    const picked: string[] = []
    const anchor = document.createElement('button')
    document.body.appendChild(anchor)
    wrapper = mountWithProviders(
      ResponsiveMenu,
      { open: true, anchor: { type: 'element', element: () => anchor }, ariaLabel: 'Display' },
      { default: () => h(MenuList, { items: ITEMS, onSelect: (id: string) => picked.push(id) }) },
    )
    await settle()
    const menu = document.body.querySelector<HTMLElement>('[role="menu"]')!
    const rows = Array.from(menu.querySelectorAll<HTMLElement>('[role^="menuitem"]'))
    expect(rows.map((r) => [r.getAttribute('role'), r.getAttribute('aria-checked')])).toEqual([
      ['menuitemradio', 'true'],
      ['menuitemradio', 'false'],
      ['menuitem', null],
    ])
    expect(menu.querySelector('[role="separator"]')).not.toBeNull()
    rows[1].click()
    expect(picked).toEqual(['updated'])
  })

  it('keeps the roles outside a Reka menu', () => {
    wrapper = mountWithProviders(MenuList, { items: ITEMS })
    const rows = wrapper.findAll('[role^="menuitem"]')
    expect(rows.map((r) => r.attributes('role'))).toEqual(['menuitemradio', 'menuitemradio', 'menuitem'])
    expect(rows[0].attributes('aria-checked')).toBe('true')
  })
})
