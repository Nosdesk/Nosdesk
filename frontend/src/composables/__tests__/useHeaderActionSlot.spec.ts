import { afterEach, describe, expect, it } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import { headerActionSlotFilled, useHeaderActionSlot } from '@/composables/useHeaderActionSlot'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const Host = defineComponent({
  props: { active: { type: Boolean, default: false } },
  setup(props) {
    useHeaderActionSlot(() => props.active)
    return () => h('div')
  },
})

describe('useHeaderActionSlot', () => {
  it('follows the view while mounted and clears when it unmounts', async () => {
    expect(headerActionSlotFilled.value).toBe(false)

    wrapper = mount(Host, { props: { active: true } })
    expect(headerActionSlotFilled.value).toBe(true)

    await wrapper.setProps({ active: false })
    expect(headerActionSlotFilled.value).toBe(false)

    await wrapper.setProps({ active: true })
    expect(headerActionSlotFilled.value).toBe(true)

    wrapper.unmount()
    wrapper = null
    expect(headerActionSlotFilled.value).toBe(false)
  })
})
