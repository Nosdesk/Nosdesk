import { describe, expect, it } from 'vitest'
import { defineComponent, h, nextTick } from 'vue'
import { mount } from '@vue/test-utils'
import { ConfigProvider, TooltipProvider } from 'reka-ui'
import IconButton from '../IconButton.vue'

// The app wraps everything in these two providers (App.vue); a bare mount
// would throw on the missing tooltip context.
function mountIconButton(props: Record<string, unknown> = {}) {
  const Host = defineComponent({
    setup() {
      return () =>
        h(ConfigProvider, {}, () =>
          h(TooltipProvider, { delayDuration: 0 }, () =>
            h(IconButton, { label: 'Delete', icon: 'trash', ...props }),
          ),
        )
    },
  })
  return mount(Host, { attachTo: document.body })
}

describe('IconButton', () => {
  it('names the button and drops the native title', () => {
    const w = mountIconButton()
    const button = w.get('button')
    expect(button.attributes('aria-label')).toBe('Delete')
    expect(button.attributes('title')).toBeUndefined()
    expect(button.attributes('data-icon-only')).toBeDefined()
    w.unmount()
  })

  it('passes listeners and classes through to the button, not the wrapper', async () => {
    let clicks = 0
    const w = mountIconButton({ class: 'extra', onClick: () => clicks++ })
    const button = w.get('button')
    expect(button.classes()).toContain('extra')
    await button.trigger('click')
    expect(clicks).toBe(1)
    w.unmount()
  })

  it('opens a tooltip on focus that describes the trigger', async () => {
    const w = mountIconButton()
    const button = w.get('button')
    await button.trigger('focus')
    await nextTick()
    await new Promise((r) => setTimeout(r, 10))
    const tooltip = document.body.querySelector('[role="tooltip"]')
    expect(tooltip?.textContent).toBe('Delete')
    expect(button.attributes('aria-describedby')).toBe(tooltip?.id)
    w.unmount()
  })

  it('renders no tooltip when opted out', async () => {
    const w = mountIconButton({ tooltip: false })
    await w.get('button').trigger('focus')
    await nextTick()
    expect(document.body.querySelector('[role="tooltip"]')).toBeNull()
    w.unmount()
  })
})
