import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { h, nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import ResponsivePanel from '@/components/common/ResponsivePanel.vue'

const settle = () => new Promise((r) => setTimeout(r, 20))

let wrapper: VueWrapper | null = null
let mobile = false

beforeEach(() => {
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

function mountPanel() {
  const closes: number[] = []
  wrapper = mountWithProviders(
    ResponsivePanel,
    { open: true, title: 'Revision history', onClose: () => closes.push(1) },
    { default: () => h('input', { id: 'inside' }) },
  )
  return { closes }
}

describe('ResponsivePanel on desktop', () => {
  it('is a named side panel with a named close button', async () => {
    const { closes } = mountPanel()
    await nextTick()
    const aside = wrapper!.get('aside')
    expect(aside.attributes('role')).toBe('complementary')
    expect(aside.attributes('aria-label')).toBe('Revision history')
    expect(document.body.querySelector('[role="dialog"]')).toBeNull()
    await wrapper!.get('button[aria-label="common-panel-close"]').trigger('click')
    expect(closes).toHaveLength(1)
  })
})

describe('ResponsivePanel on a phone', () => {
  beforeEach(() => {
    mobile = true
  })

  it('is a modal sheet named by its title that traps focus and closes on Escape', async () => {
    const { closes } = mountPanel()
    await settle()
    expect(wrapper!.find('aside').exists()).toBe(false)
    const dialog = document.body.querySelector<HTMLElement>('[role="dialog"]')
    expect(dialog).not.toBeNull()
    expect(dialog!.getAttribute('aria-modal')).toBe('true')
    expect(document.getElementById(dialog!.getAttribute('aria-labelledby')!)?.textContent).toBe('Revision history')
    // Focus lands inside the sheet, on the first tabbable.
    expect(document.activeElement?.id).toBe('inside')
    dialog!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await nextTick()
    expect(closes).toHaveLength(1)
  })
})
