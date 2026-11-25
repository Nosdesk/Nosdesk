import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import InlineEdit from '@/components/common/InlineEdit.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  // A heading unmounted while it has focus leaves jsdom's focus state behind,
  // and the next test's first focus() then never reaches its handlers.
  ;(document.activeElement as HTMLElement | null)?.blur()
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

describe('InlineEdit multiline (the page title on a phone)', () => {
  function textarea(): HTMLTextAreaElement {
    return wrapper!.get('textarea').element as HTMLTextAreaElement
  }

  // A heading that already has focus (handed back after an edit) opens on a
  // tap; one that doesn't opens on focus.
  async function openArea(): Promise<HTMLTextAreaElement> {
    const preview = wrapper!.get('[tabindex="0"]').element as HTMLElement
    if (document.activeElement === preview) preview.click()
    else preview.focus()
    await settle()
    await settle()
    const el = textarea()
    expect(el.hidden).toBe(false)
    expect(document.activeElement).toBe(el)
    return el
  }

  function typeArea(el: HTMLTextAreaElement, text: string) {
    el.value = text
    el.setSelectionRange(text.length, text.length)
    el.dispatchEvent(new Event('input', { bubbles: true }))
  }

  it('shows the title as an h1 and edits it in a textarea, not an input', async () => {
    wrapper = mountWithProviders(InlineEdit, { modelValue: 'Printer jam', multiline: true, as: 'h1', placeholder: 'Title' })
    await nextTick()
    const heading = wrapper.get('h1')
    expect(heading.attributes('tabindex')).toBe('0')
    expect(heading.text()).toBe('Printer jam')
    expect(wrapper.find('input').exists()).toBe(false)
    const el = await openArea()
    expect(el.getAttribute('aria-label')).toBe('Title')
    expect(el.value).toBe('Printer jam')
  })

  it('puts the caret at the end instead of selecting the title', async () => {
    wrapper = mountWithProviders(InlineEdit, { modelValue: 'Printer jam on level 3', multiline: true })
    await nextTick()
    const el = await openArea()
    expect(el.selectionStart).toBe('Printer jam on level 3'.length)
    expect(el.selectionEnd).toBe('Printer jam on level 3'.length)
  })

  it('saves on Enter without typing a line break, once and only on a change', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(InlineEdit, {
      modelValue: 'Printer jam',
      multiline: true,
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    let el = await openArea()
    let enter = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })
    el.dispatchEvent(enter)
    await settle()
    expect(enter.defaultPrevented).toBe(true)
    expect(updates).toEqual([])
    el = await openArea()
    typeArea(el, 'Printer jammed')
    enter = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })
    el.dispatchEvent(enter)
    await settle()
    expect(enter.defaultPrevented).toBe(true)
    expect(updates).toEqual(['Printer jammed'])
    expect(textarea().hidden).toBe(true)
  })

  it('saves on blur', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(InlineEdit, {
      modelValue: 'Printer jam',
      multiline: true,
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    const el = await openArea()
    typeArea(el, 'Printer jam fixed')
    const elsewhere = document.createElement('button')
    document.body.appendChild(elsewhere)
    elsewhere.focus()
    await settle()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await settle()
    elsewhere.remove()
    expect(updates).toEqual(['Printer jam fixed'])
  })

  it('cancels on Escape, restoring the title', async () => {
    const updates: string[] = []
    const previews: string[] = []
    wrapper = mountWithProviders(InlineEdit, {
      modelValue: 'Printer jam',
      multiline: true,
      as: 'h1',
      'onUpdate:modelValue': (v: string) => updates.push(v),
      onPreview: (v: string) => previews.push(v),
    })
    await nextTick()
    const el = await openArea()
    typeArea(el, 'Prin')
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await settle()
    expect(updates).toEqual([])
    expect(previews.at(-1)).toBe('Printer jam')
    expect(wrapper.get('h1').text()).toBe('Printer jam')
  })

  it('turns a pasted line break into a space, in the field and in what it saves', async () => {
    const updates: string[] = []
    const previews: string[] = []
    wrapper = mountWithProviders(InlineEdit, {
      modelValue: 'Printer',
      multiline: true,
      'onUpdate:modelValue': (v: string) => updates.push(v),
      onPreview: (v: string) => previews.push(v),
    })
    await nextTick()
    const el = await openArea()
    typeArea(el, 'Printer jam\r\n  on level 3\n')
    await settle()
    expect(el.value).toBe('Printer jam on level 3 ')
    expect(previews).toEqual(['Printer jam on level 3 '])
    expect(wrapper.get('[aria-hidden="true"].invisible').text()).toContain('Printer jam on level 3')
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    await settle()
    // The field keeps the space as it was typed; the saved title is trimmed.
    expect(updates).toEqual(['Printer jam on level 3'])
  })

  it('shows the placeholder for an empty title and opens the editor empty', async () => {
    wrapper = mountWithProviders(InlineEdit, {
      modelValue: '',
      multiline: true,
      as: 'h1',
      placeholder: 'Ticket title',
      emptyLabel: 'Untitled ticket',
    })
    await nextTick()
    expect(wrapper.get('h1').text()).toBe('Ticket title')
    expect(wrapper.get('h1').attributes('aria-label')).toBe('Untitled ticket')
    const el = await openArea()
    expect(el.value).toBe('')
  })

  it('names a titled heading by its text and gives it no tooltip copy', async () => {
    wrapper = mountWithProviders(InlineEdit, {
      modelValue: 'Printer jam',
      multiline: true,
      as: 'h1',
      maxLines: 2,
      emptyLabel: 'Untitled ticket',
    })
    await nextTick()
    expect(wrapper.get('h1').attributes('aria-label')).toBeUndefined()
    expect(wrapper.get('h1').attributes('title')).toBeUndefined()
  })

  it('saves on a blur to nowhere (a phone keyboard dismissed) and hands focus back to the heading', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(InlineEdit, {
      modelValue: 'Printer jam',
      multiline: true,
      as: 'h1',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    const el = await openArea()
    typeArea(el, 'Printer jam cleared')
    el.blur()
    await settle()
    await settle()
    expect(updates).toEqual(['Printer jam cleared'])
    expect(textarea().hidden).toBe(true)
    // Focus is back on the heading, and the editor did not reopen.
    expect(document.activeElement).toBe(wrapper.get('h1').element)
    await settle()
    expect(textarea().hidden).toBe(true)
  })

  it('hands focus back to the heading after Enter and Escape without reopening', async () => {
    wrapper = mountWithProviders(InlineEdit, { modelValue: 'Printer jam', multiline: true, as: 'h1' })
    await nextTick()
    let el = await openArea()
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    await settle()
    await settle()
    expect(document.activeElement).toBe(wrapper.get('h1').element)
    expect(textarea().hidden).toBe(true)

    // Leave and come back in, the way a keyboard user would.
    ;(document.activeElement as HTMLElement).blur()
    el = await openArea()
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await settle()
    await settle()
    expect(document.activeElement).toBe(wrapper.get('h1').element)
    expect(textarea().hidden).toBe(true)
  })

  it('leaves focus where it went when the edit ends by moving elsewhere', async () => {
    wrapper = mountWithProviders(InlineEdit, { modelValue: 'Printer jam', multiline: true, as: 'h1' })
    await nextTick()
    await openArea()
    const elsewhere = document.createElement('button')
    document.body.appendChild(elsewhere)
    elsewhere.focus()
    await settle()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await settle()
    expect(textarea().hidden).toBe(true)
    expect(document.activeElement).toBe(elsewhere)
    elsewhere.remove()
  })

  it('lets an input method take Enter, including Safari\'s keyCode 229 after compositionend', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(InlineEdit, {
      modelValue: 'Printer',
      multiline: true,
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    const el = await openArea()
    typeArea(el, 'Printer jam')
    for (const init of [{ isComposing: true }, { keyCode: 229 }]) {
      const enter = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true, ...init })
      el.dispatchEvent(enter)
      await settle()
      expect(enter.defaultPrevented).toBe(false)
      expect(updates).toEqual([])
      expect(textarea().hidden).toBe(false)
    }
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    await settle()
    expect(updates).toEqual(['Printer jam'])
  })
})
