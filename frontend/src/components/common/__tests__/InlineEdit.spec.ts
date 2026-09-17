import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import InlineEdit from '@/components/common/InlineEdit.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const settle = async () => {
  await nextTick()
  await nextTick()
}

function input(): HTMLInputElement {
  return wrapper!.get('input').element as HTMLInputElement
}

function typeText(el: HTMLInputElement, text: string) {
  el.value = text
  el.dispatchEvent(new Event('input', { bubbles: true }))
}

async function open(): Promise<HTMLInputElement> {
  const preview = wrapper!.get('[tabindex="0"]').element as HTMLElement
  preview.focus()
  await settle()
  const el = input()
  expect(el.hidden).toBe(false)
  expect(document.activeElement).toBe(el)
  return el
}

describe('InlineEdit', () => {
  it('shows the value as a focusable preview and opens the editor on focus', async () => {
    wrapper = mountWithProviders(InlineEdit, { modelValue: 'Printer jam', placeholder: 'Title' })
    await nextTick()
    const preview = wrapper.get('[tabindex="0"]')
    expect(preview.text()).toBe('Printer jam')
    expect(input().hidden).toBe(true)
    const el = await open()
    expect(el.getAttribute('aria-label')).toBe('Title')
    expect(el.value).toBe('Printer jam')
  })

  it('commits once on Enter, only when the value changed, and previews each keystroke', async () => {
    const updates: string[] = []
    const previews: string[] = []
    wrapper = mountWithProviders(InlineEdit, {
      modelValue: 'Printer jam',
      'onUpdate:modelValue': (v: string) => updates.push(v),
      onPreview: (v: string) => previews.push(v),
    })
    await nextTick()
    let el = await open()
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await settle()
    expect(updates).toEqual([])
    el = await open()
    typeText(el, 'Printer jammed')
    expect(previews).toEqual(['Printer jammed'])
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await settle()
    expect(updates).toEqual(['Printer jammed'])
    expect(input().hidden).toBe(true)
  })

  it('cancels on Escape, restoring the original and previewing it', async () => {
    const updates: string[] = []
    const previews: string[] = []
    wrapper = mountWithProviders(InlineEdit, {
      modelValue: 'Printer jam',
      'onUpdate:modelValue': (v: string) => updates.push(v),
      onPreview: (v: string) => previews.push(v),
    })
    await nextTick()
    const el = await open()
    typeText(el, 'Prin')
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await settle()
    expect(updates).toEqual([])
    expect(previews.at(-1)).toBe('Printer jam')
    expect(wrapper.get('[tabindex="0"]').text()).toBe('Printer jam')
  })

  it('keeps the draft when the parent echoes or changes the value mid-edit', async () => {
    const previews: string[] = []
    wrapper = mountWithProviders(InlineEdit, {
      modelValue: 'Printer jam',
      onPreview: (v: string) => previews.push(v),
    })
    await nextTick()
    const el = await open()
    typeText(el, 'Printer jammed again')
    await wrapper.setProps({ modelValue: 'Printer jammed again' })
    await settle()
    expect(el.value).toBe('Printer jammed again')
    await wrapper.setProps({ modelValue: 'Someone else typed' })
    await settle()
    expect(el.value).toBe('Printer jammed again')
  })

  it('is inert when editing is off', async () => {
    wrapper = mountWithProviders(InlineEdit, { modelValue: 'Serial', canEdit: false })
    await nextTick()
    expect(wrapper.find('[tabindex="0"]').exists()).toBe(false)
    const preview = wrapper.get('[tabindex="-1"]').element as HTMLElement
    preview.dispatchEvent(new FocusEvent('focusin', { bubbles: true }))
    await settle()
    expect(input().hidden).toBe(true)
  })
})
