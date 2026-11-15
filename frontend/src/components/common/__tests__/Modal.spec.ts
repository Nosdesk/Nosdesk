import { afterEach, describe, expect, it, vi } from 'vitest'
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

// The sheet presentation: below `md` with a coarse pointer. Specs that
// drive the swipe install this mock; everything else keeps the harness
// default, under which the gesture is off.
function presentAs(mode: 'phone' | 'desktop') {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes('min-width') ? mode === 'desktop' : query.includes('pointer') && mode === 'phone',
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    onchange: null,
    dispatchEvent: () => false,
  }))
}

const SHEET_HEIGHT = 600

function pointer(type: string, target: Element, y: number) {
  target.dispatchEvent(
    new PointerEvent(type, {
      pointerId: 7,
      pointerType: 'mouse',
      isPrimary: true,
      button: 0,
      clientX: 180,
      clientY: y,
      bubbles: true,
    }),
  )
}

/** A slow drag down from the header: press, one move past the slop, one
 *  to `distance`, a hold, release. The hold outlasts the velocity window
 *  so distance alone decides, as for a finger that stops before letting
 *  go. jsdom lays nothing out, so the panel reports a phone sheet's
 *  height, and the glide is a CSS transition it never runs, so the spec
 *  ends it by hand. */
async function swipeDown(el: HTMLElement, distance: number) {
  el.getBoundingClientRect = () => ({ height: SHEET_HEIGHT }) as DOMRect
  const handle = el.querySelector<HTMLElement>('.modal-header__title')!
  pointer('pointerdown', handle, 40)
  await settle()
  pointer('pointermove', handle, 60)
  await settle()
  pointer('pointermove', handle, 40 + distance)
  await new Promise((r) => setTimeout(r, 120))
  pointer('pointermove', handle, 40 + distance)
  pointer('pointerup', handle, 40 + distance)
  el.dispatchEvent(new TransitionEvent('transitionend', { propertyName: 'transform' }))
  await settle()
}

describe('Modal as a sheet on a phone', () => {
  const originalMatchMedia = window.matchMedia
  afterEach(() => {
    window.matchMedia = originalMatchMedia
  })

  it('shows a decorative grabber and closes on a swipe down from the header', async () => {
    presentAs('phone')
    const { closes } = mountModal({ show: true })
    await settle()
    const grabber = dialog()!.querySelector('.modal-grabber')
    expect(grabber?.getAttribute('aria-hidden')).toBe('true')
    await swipeDown(dialog()!, SHEET_HEIGHT / 2)
    expect(closes).toHaveLength(1)
  })

  it('springs back from a short swipe without closing', async () => {
    presentAs('phone')
    const { closes } = mountModal({ show: true })
    await settle()
    await swipeDown(dialog()!, 50)
    expect(closes).toHaveLength(0)
    expect(dialog()).not.toBeNull()
  })

  it('ignores the swipe in alert mode, as it ignores the backdrop', async () => {
    presentAs('phone')
    const { closes } = mountModal({ show: true, alert: true })
    await settle()
    const el = document.body.querySelector<HTMLElement>('[role="alertdialog"]')
    expect(el).not.toBeNull()
    await swipeDown(el!, SHEET_HEIGHT / 2)
    expect(closes).toHaveLength(0)
  })

  it('ignores the swipe on the desktop card', async () => {
    presentAs('desktop')
    const { closes } = mountModal({ show: true })
    await settle()
    await swipeDown(dialog()!, SHEET_HEIGHT / 2)
    expect(closes).toHaveLength(0)
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
