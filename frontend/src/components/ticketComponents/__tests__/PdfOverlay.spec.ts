import { afterEach, describe, expect, it, vi } from 'vitest'
import { toValue } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'

// The viewer stand-in fails with `viewer.next` when it mounts, if set.
const viewer = vi.hoisted(() => ({ next: null as unknown, mounts: 0, src: '' }))
vi.mock('@/components/ticketComponents/PDFViewer.vue', () => ({
  default: {
    props: ['src', 'filename', 'initialPage'],
    emits: ['error'],
    mounted() {
      const self = this as unknown as { src: string; $emit: (e: string, v: unknown) => void }
      viewer.mounts++
      viewer.src = self.src
      const error = viewer.next
      viewer.next = null
      if (error) self.$emit('error', error)
    },
    template: '<div data-test="pdf" :data-src="src" :data-page="initialPage" />',
  },
}))
const toast = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }))
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => toast }))

import PdfOverlay from '@/components/ticketComponents/PdfOverlay.vue'
import { activeClaim } from '@/composables/usePageTitle'
import { mountWithProviders } from '@/test/mountWithProviders'
import { PdfFetchError, PdfPasswordCancelled } from '@/utils/pdfjs'

const stored = '/uploads/tickets/7/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb_invoice.pdf'
const settle = () => new Promise((r) => setTimeout(r, 30))

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  viewer.mounts = 0
  vi.clearAllMocks()
})

async function mountAt(path: string) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:rest(.*)*', component: { render: () => null } }],
  })
  await router.push(path)
  await router.isReady()
  wrapper = mountWithProviders(PdfOverlay, {}, {}, [router])
  await flushPromises()
  await settle()
  return router
}

const dialog = () => document.body.querySelector<HTMLElement>('[role="dialog"]')
const button = (label: string) =>
  [...document.body.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find(
    (b) => b.getAttribute('aria-label') === label || b.textContent?.trim() === label,
  )

describe('PdfOverlay', () => {
  it('stays closed on a page with no PDF', async () => {
    await mountAt('/tickets/12')
    expect(dialog()).toBeNull()
  })

  it('shows the linked PDF over the page, named in the dialog and the tab', async () => {
    await mountAt(`/tickets/12?pdf=${encodeURIComponent(stored)}&page=3`)
    expect(dialog()).not.toBeNull()
    const title = document.getElementById(dialog()!.getAttribute('aria-labelledby')!)
    expect(title?.textContent?.trim()).toBe('invoice.pdf')
    expect(toValue(activeClaim.value?.claim.title)).toBe('invoice.pdf')
    // Fetched through the authenticated file endpoint, on the linked page.
    const pdf = document.body.querySelector('[data-test="pdf"]')!
    expect(pdf.getAttribute('data-src')).toBe(stored.replace('/uploads/', '/api/files/'))
    expect(pdf.getAttribute('data-page')).toBe('3')
  })

  it('closes on Escape, leaving the page as it was', async () => {
    const router = await mountAt(`/tickets/12?pdf=${encodeURIComponent(stored)}`)
    dialog()!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await settle()
    expect(router.currentRoute.value.fullPath).toBe('/tickets/12')
    expect(dialog()).toBeNull()
    expect(activeClaim.value).toBeNull()
  })

  it('copies a link to the page with the PDF open', async () => {
    const writeText = vi.fn(async () => {})
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    await mountAt(`/tickets/12?pdf=${encodeURIComponent(stored)}`)
    button('pdf-viewer-copy-link')!.click()
    await flushPromises()
    expect(writeText).toHaveBeenCalledWith(window.location.href)
    expect(toast.success).toHaveBeenCalledWith('pdf-viewer-link-copied')
  })

  it('says so for a link to a file it may not open', async () => {
    await mountAt(`/tickets/12?pdf=${encodeURIComponent('https://evil.example/x.pdf')}`)
    expect(dialog()!.textContent).toContain('pdf-viewer-error-no-source')
    expect(document.body.querySelector('[data-test="pdf"]')).toBeNull()
  })

  it('explains a failed load, and tries again on request', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    viewer.next = new PdfFetchError(null, 'Load failed')
    await mountAt(`/tickets/12?pdf=${encodeURIComponent(stored)}`)
    expect(dialog()!.querySelector('[data-testid="pdf-viewer-failure"] h3')!.textContent).toBe('pdf-viewer-network-title')
    button('pdf-viewer-try-again')!.click()
    await settle()
    expect(viewer.mounts).toBe(2)
    expect(document.body.querySelector('[data-test="pdf"]')).not.toBeNull()
  })

  it('offers the password again after the prompt is closed', async () => {
    viewer.next = new PdfPasswordCancelled()
    await mountAt(`/tickets/12?pdf=${encodeURIComponent(stored)}`)
    button('pdf-viewer-locked-enter-password')!.click()
    await settle()
    expect(viewer.mounts).toBe(2)
  })
})
