import { afterEach, describe, expect, it } from 'vitest'
import { h, nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import Modal from '@/components/Modal.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'

const settle = () => new Promise((r) => setTimeout(r, 20))

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

function mountModal(props: Record<string, unknown>, body?: () => unknown, footer?: () => unknown) {
  const closes: number[] = []
  const opener = document.createElement('button')
  opener.id = 'opener'
  document.body.appendChild(opener)
  opener.focus()
  const outside = document.createElement('main')
  outside.id = 'page'
  document.body.appendChild(outside)
  const slots: Record<string, () => unknown> = { default: body ?? (() => h('input', { id: 'name' })) }
  if (footer) slots.footer = footer
  wrapper = mountWithProviders(
    Modal,
    { title: 'Rename ticket', ...props, onClose: () => closes.push(1) },
    slots,
  )
  return { closes, opener, outside }
}

const dialog = () => document.body.querySelector<HTMLElement>('[role="dialog"]')

describe('Modal', () => {
  it('renders an accessible dialog named by its title and closes on Escape', async () => {
    const { closes } = mountModal({ show: true })
    await settle()
    const el = dialog()
    expect(el).not.toBeNull()
    // Modality is conveyed by hiding the rest of the page (next test);
    // aria-modal says so to readers that look for it.
    expect(el?.getAttribute('aria-modal')).toBe('true')
    const title = document.getElementById(el!.getAttribute('aria-labelledby')!)
    expect(title?.textContent).toBe('Rename ticket')
    expect(el?.hasAttribute('aria-describedby')).toBe(false)
    el?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await nextTick()
    expect(closes).toHaveLength(1)
  })

  it('hides the rest of the page from assistive tech while open, except live regions', async () => {
    const live = document.createElement('div')
    live.setAttribute('aria-live', 'polite')
    document.body.appendChild(live)
    const { outside } = mountModal({ show: true })
    await settle()
    expect(outside.getAttribute('aria-hidden')).toBe('true')
    expect(live.getAttribute('aria-hidden')).toBeNull()
  })

  it('closes from the close button and reports a description when given', async () => {
    const { closes } = mountModal({ show: true, description: 'Pick a new name.' })
    await settle()
    const el = dialog()!
    expect(document.getElementById(el.getAttribute('aria-describedby')!)?.textContent?.trim()).toBe(
      'Pick a new name.',
    )
    el.querySelector<HTMLButtonElement>('.modal-header__close')?.click()
    await nextTick()
    expect(closes).toHaveLength(1)
  })

  it('gives initial focus to an autofocus element inside the body', async () => {
    mountModal({ show: true }, () => [h('input', { id: 'first' }), h('input', { id: 'wanted', autofocus: '' })])
    await settle()
    expect(document.activeElement?.id).toBe('wanted')
  })

  it('renders nothing while hidden', async () => {
    mountModal({ show: false })
    await settle()
    expect(dialog()).toBeNull()
  })
})

describe('ConfirmModal', () => {
  it('is an alert dialog that opens on Cancel and emits confirm from the action', async () => {
    const events: string[] = []
    wrapper = mountWithProviders(ConfirmModal, {
      show: true,
      title: 'Delete ticket?',
      message: 'This cannot be undone.',
      confirmLabel: 'Delete',
      cancelLabel: 'Keep',
      variant: 'danger',
      onConfirm: () => events.push('confirm'),
      onClose: () => events.push('close'),
    })
    await settle()
    const el = document.body.querySelector<HTMLElement>('[role="alertdialog"]')
    expect(el).not.toBeNull()
    expect(document.activeElement?.textContent?.trim()).toBe('Keep')
    const buttons = Array.from(el!.querySelectorAll('button'))
    buttons.find((b) => b.textContent?.trim() === 'Delete')?.click()
    buttons.find((b) => b.textContent?.trim() === 'Keep')?.click()
    await nextTick()
    expect(events).toEqual(['confirm', 'close'])
  })

  it('names its actions from the catalogue when the consumer does not', async () => {
    wrapper = mountWithProviders(ConfirmModal, { show: true, title: 'Delete ticket?', message: 'Sure?' })
    await settle()
    const buttons = Array.from(document.body.querySelectorAll('[role="alertdialog"] button'))
    expect(buttons.map((b) => b.textContent?.trim()).filter(Boolean)).toEqual(['common-cancel', 'common-confirm'])
  })
})
