import { afterEach, describe, expect, it } from 'vitest'
import { h, nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import Popover from '@/components/common/Popover.vue'

const settle = () => new Promise((r) => setTimeout(r, 20))

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

function mountPopover(props: Record<string, unknown> = {}) {
  const closes: number[] = []
  const anchor = document.createElement('button')
  anchor.id = 'anchor'
  anchor.textContent = 'open'
  document.body.appendChild(anchor)
  const outside = document.createElement('button')
  outside.id = 'outside'
  document.body.appendChild(outside)
  wrapper = mountWithProviders(
    Popover,
    {
      open: true,
      anchor: { type: 'element', element: () => anchor },
      onClose: () => closes.push(1),
      ...props,
    },
    { default: () => [h('button', { id: 'first' }, 'one'), h('button', { id: 'second' }, 'two')] },
  )
  return { closes, anchor, outside }
}

const content = () => document.body.querySelector<HTMLElement>('.popover-inner')

function pointerDown(target: Element) {
  const ev = new Event('pointerdown', { bubbles: true, cancelable: true }) as PointerEvent
  Object.defineProperty(ev, 'pointerType', { value: 'mouse' })
  Object.defineProperty(ev, 'button', { value: 0 })
  target.dispatchEvent(ev)
}

describe('Popover', () => {
  it('renders the content with the given role and focuses inside', async () => {
    mountPopover({ role: 'menu', ariaLabel: 'Actions' })
    await settle()
    const el = content()
    expect(el).not.toBeNull()
    expect(el?.getAttribute('role')).toBe('menu')
    expect(el?.getAttribute('aria-label')).toBe('Actions')
    expect(el?.contains(document.activeElement)).toBe(true)
  })

  it('leaves focus alone when autoFocus is off', async () => {
    const { anchor } = mountPopover({ autoFocus: false })
    anchor.focus()
    await settle()
    expect(document.activeElement).toBe(anchor)
  })

  it('closes on Escape and on a pointerdown outside, but not on the anchor', async () => {
    const { closes, anchor, outside } = mountPopover()
    await settle()
    pointerDown(anchor)
    await nextTick()
    expect(closes).toHaveLength(0)
    pointerDown(outside)
    await nextTick()
    expect(closes).toHaveLength(1)
    content()?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await nextTick()
    expect(closes).toHaveLength(2)
  })

  it('stays open when focus moves to the anchor, closes when it moves elsewhere', async () => {
    // Chrome focuses a button on mousedown, so a click on the toggle
    // reaches Reka as focus leaving the surface before the click lands.
    const { closes, anchor, outside } = mountPopover()
    await settle()
    anchor.focus()
    await settle()
    expect(closes).toHaveLength(0)
    outside.focus()
    await settle()
    expect(closes).toHaveLength(1)
  })

  it('positions against a viewport point when the anchor is a point', async () => {
    mountPopover({ anchor: { type: 'point', x: 40, y: 50 } })
    await settle()
    expect(content()).not.toBeNull()
  })

  it('renders nothing while closed', async () => {
    mountPopover({ open: false })
    await settle()
    expect(content()).toBeNull()
  })
})
