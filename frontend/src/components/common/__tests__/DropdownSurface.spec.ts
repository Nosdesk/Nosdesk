import { afterEach, describe, expect, it } from 'vitest'
import { h } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import DropdownSurface from '@/components/common/DropdownSurface.vue'

const settle = () => new Promise((r) => setTimeout(r, 20))

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

function mountSurface() {
  const closes: number[] = []
  const anchor = document.createElement('button')
  anchor.id = 'anchor'
  document.body.appendChild(anchor)
  const outside = document.createElement('button')
  outside.id = 'outside'
  document.body.appendChild(outside)
  wrapper = mountWithProviders(
    DropdownSurface,
    {
      open: true,
      anchor: { type: 'element', element: () => anchor },
      onClose: () => closes.push(1),
    },
    { default: () => [h('div', { role: 'menuitem', tabindex: 0 }, 'one')] },
  )
  return { closes, anchor, outside }
}

describe('DropdownSurface', () => {
  it('stays open when focus moves to the anchor, closes when it moves elsewhere', async () => {
    // The user menu's toggle button: Chrome focuses it on mousedown, and
    // that focusin must not read as leaving the menu.
    const { closes, anchor, outside } = mountSurface()
    await settle()
    anchor.focus()
    await settle()
    expect(closes).toHaveLength(0)
    outside.focus()
    await settle()
    expect(closes).toHaveLength(1)
  })
})
