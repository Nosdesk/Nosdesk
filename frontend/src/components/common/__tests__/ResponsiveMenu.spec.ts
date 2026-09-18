import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { h, nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import MenuList from '@/components/common/MenuList.vue'
import MenuItem from '@/components/common/MenuItem.vue'

const settle = () => new Promise((r) => setTimeout(r, 20))

let wrapper: VueWrapper | null = null
let mobile = false

beforeEach(() => {
  // useResponsiveSheet resolves the breakpoint synchronously from matchMedia.
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes('min-width') ? !mobile : mobile,
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

function mountMenu(props: Record<string, unknown>, content: () => unknown) {
  const closes: number[] = []
  const anchor = document.createElement('button')
  anchor.id = 'anchor'
  document.body.appendChild(anchor)
  wrapper = mountWithProviders(
    ResponsiveMenu,
    { open: true, anchor: { type: 'element', element: () => anchor }, onClose: () => closes.push(1), ...props },
    { default: content },
  )
  return { closes, anchor }
}

const key = (el: Element | null, k: string) =>
  el?.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }))

describe('ResponsiveMenu on desktop', () => {
  beforeEach(() => {
    mobile = false
  })

  it('renders a real menu whose rows are Reka menu items with roving focus', async () => {
    const picked: string[] = []
    mountMenu({ ariaLabel: 'Actions' }, () =>
      h(MenuList, {
        items: [
          { id: 'rename', label: 'Rename' },
          { id: 'archive', label: 'Archive', divider: true },
          { id: 'delete', label: 'Delete', danger: true },
        ],
        onSelect: (id: string) => picked.push(id),
      }),
    )
    await settle()
    const menu = document.body.querySelector<HTMLElement>('[role="menu"]')
    expect(menu?.getAttribute('aria-label')).toBe('Actions')
    const items = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitem"]'))
    expect(items.map((i) => i.textContent?.trim())).toEqual(['Rename', 'Archive', 'Delete'])
    expect(items[0].tagName).toBe('BUTTON')

    // Arrow keys walk rows; Enter activates the consumer's click handler.
    key(menu, 'ArrowDown')
    await nextTick()
    expect(document.activeElement).toBe(items[0])
    key(items[0], 'ArrowDown')
    await nextTick()
    expect(document.activeElement).toBe(items[1])
    key(items[1], 'Enter')
    await nextTick()
    expect(picked).toEqual(['archive'])
  })

  it('closes on Escape and reports checkbox rows with aria-checked', async () => {
    const { closes } = mountMenu({}, () => [
      h(MenuItem, { role: 'menuitemcheckbox', checked: true }, () => 'Assignee'),
      h(MenuItem, { role: 'menuitemcheckbox', checked: false }, () => 'Priority'),
    ])
    await settle()
    const boxes = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitemcheckbox"]'))
    expect(boxes.map((b) => b.getAttribute('aria-checked'))).toEqual(['true', 'false'])
    key(document.body.querySelector('[role="menu"]'), 'Escape')
    await nextTick()
    expect(closes).toHaveLength(1)
  })

  it('renders a dialog surface for panels that are not menus', async () => {
    mountMenu({ role: 'dialog', ariaLabel: 'Display' }, () => h('input', { id: 'search' }))
    await settle()
    expect(document.body.querySelector('[role="menu"]')).toBeNull()
    const dialog = document.body.querySelector<HTMLElement>('[role="dialog"]')
    expect(dialog?.getAttribute('aria-label')).toBe('Display')
    expect(dialog?.querySelector('#search')).not.toBeNull()
  })
})

describe('ResponsiveMenu on a phone', () => {
  beforeEach(() => {
    mobile = true
  })

  it('renders a modal bottom sheet named by its title, with a menu body', async () => {
    const { closes } = mountMenu({ title: 'Ticket actions' }, () =>
      h(MenuList, { items: [{ id: 'a', label: 'Assign' }] }),
    )
    await settle()
    const dialog = document.body.querySelector<HTMLElement>('[role="dialog"]')
    expect(dialog).not.toBeNull()
    expect(document.getElementById(dialog!.getAttribute('aria-labelledby')!)?.textContent).toBe(
      'Ticket actions',
    )
    expect(dialog?.getAttribute('aria-modal')).toBe('true')
    const body = dialog!.querySelector('[role="menu"]')
    expect(body?.querySelector('[role="menuitem"]')?.textContent?.trim()).toBe('Assign')
    dialog!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await nextTick()
    expect(closes).toHaveLength(1)
  })

  it('never renders a sheet with an empty name', async () => {
    mountMenu({}, () => h(MenuList, { items: [{ id: 'a', label: 'Assign' }] }))
    await settle()
    const dialog = document.body.querySelector<HTMLElement>('[role="dialog"]')
    expect(document.getElementById(dialog!.getAttribute('aria-labelledby')!)?.textContent).toBe('common-sheet-aria')
    expect(dialog!.querySelector('[role="menu"]')?.getAttribute('aria-label')).toBe('common-sheet-aria')
  })
})
