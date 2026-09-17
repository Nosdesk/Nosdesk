import { afterEach, describe, expect, it } from 'vitest'
import { h, nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { ToolbarButton } from 'reka-ui'
import { mountWithProviders } from '@/test/mountWithProviders'
import BulkActionBar from '@/components/common/BulkActionBar.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const key = (el: Element, k: string) =>
  el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }))

describe('BulkActionBar', () => {
  it('is a toolbar whose controls roll focus with the arrows', async () => {
    const events: string[] = []
    wrapper = mountWithProviders(
      BulkActionBar,
      {
        selectedCount: 2,
        totalCount: 9,
        onClear: () => events.push('clear'),
        onSelectAllMatching: () => events.push('all'),
      },
      {
        actions: () =>
          h(ToolbarButton, { asChild: true }, () => h('button', { type: 'button', onClick: () => events.push('act') }, 'Delete')),
      },
    )
    await nextTick()
    const toolbar = wrapper.get('[role="toolbar"]')
    expect(toolbar.attributes('aria-label')).toBe('common-bulk-actions-aria')
    const buttons = toolbar.findAll('button').map((b) => b.element as HTMLButtonElement)
    expect(buttons.map((b) => b.textContent?.trim())).toEqual([
      'bulk-bar-select-all-matching',
      'bulk-bar-clear',
      'Delete',
    ])
    // One tab stop for the group; arrows walk the controls.
    expect(toolbar.attributes('tabindex')).toBe('0')
    buttons[0].focus()
    key(buttons[0], 'ArrowRight')
    await nextTick()
    expect(document.activeElement).toBe(buttons[1])
    key(buttons[1], 'ArrowRight')
    await nextTick()
    expect(document.activeElement).toBe(buttons[2])
    buttons[2].click()
    buttons[1].click()
    buttons[0].click()
    expect(events).toEqual(['act', 'clear', 'all'])
  })

  it('renders nothing with no selection', async () => {
    wrapper = mountWithProviders(BulkActionBar, { selectedCount: 0 })
    await nextTick()
    expect(wrapper.find('[role="toolbar"]').exists()).toBe(false)
  })
})
