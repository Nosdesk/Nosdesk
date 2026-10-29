import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import PdfPasswordPrompt from '@/components/ticketComponents/PdfPasswordPrompt.vue'

const settle = () => new Promise((r) => setTimeout(r, 20))

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

function mountPrompt(props: Record<string, unknown> = {}) {
  const submitted: string[] = []
  const cancels: number[] = []
  wrapper = mountWithProviders(PdfPasswordPrompt, {
    show: true,
    filename: 'payslip.pdf',
    reason: 'needed',
    busy: false,
    onSubmit: (password: string) => submitted.push(password),
    onCancel: () => cancels.push(1),
    ...props,
  })
  return { submitted, cancels }
}

const input = () => document.body.querySelector<HTMLInputElement>('[role="dialog"] input')!
const button = (label: string) =>
  [...document.body.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find(
    (b) => b.textContent?.trim() === label,
  )!

describe('PdfPasswordPrompt', () => {
  it('submits the typed password with Enter or the Open button', async () => {
    const { submitted } = mountPrompt()
    await settle()
    expect(button('pdf-viewer-password-open').disabled).toBe(true)

    input().value = 'hunter2'
    input().dispatchEvent(new Event('input'))
    await nextTick()
    input().form!.requestSubmit()
    await nextTick()
    expect(submitted).toEqual(['hunter2'])
  })

  it('says when the last password was wrong', async () => {
    mountPrompt({ reason: 'incorrect' })
    await settle()
    expect(document.body.querySelector('[role="dialog"]')!.textContent).toContain('pdf-viewer-password-incorrect')
    expect(input().getAttribute('aria-invalid')).toBe('true')
  })

  it('cancels from the Cancel button', async () => {
    const { cancels } = mountPrompt()
    await settle()
    button('pdf-viewer-password-cancel').click()
    expect(cancels).toHaveLength(1)
  })

  it("doesn't submit again while a password is being tried", async () => {
    const { submitted } = mountPrompt({ busy: true })
    await settle()
    expect(input().disabled).toBe(true)
    input().form!.requestSubmit()
    await nextTick()
    expect(submitted).toEqual([])
  })
})
