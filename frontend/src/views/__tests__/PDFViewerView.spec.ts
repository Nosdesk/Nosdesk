import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { toValue } from 'vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { activeClaim } from '@/composables/usePageTitle'

// The router hands the view decoded query values.
const query = vi.hoisted(() => ({ src: '/api/files/tickets/7/a.pdf', filename: 'invoice.pdf' }))
vi.mock('vue-router', () => ({
  useRoute: () => ({ query }),
  useRouter: () => ({ push: vi.fn(), back: vi.fn() }),
}))
vi.mock('@/router/navigation', () => ({ performBack: vi.fn() }))
// The viewer stand-in fails with `viewerError.next` when it mounts, if set.
const viewerError = vi.hoisted(() => ({ next: null as unknown, mounts: 0 }))
vi.mock('@/components/ticketComponents/PDFViewer.vue', () => ({
  default: {
    props: ['src', 'filename'],
    emits: ['error'],
    mounted() {
      viewerError.mounts++
      const error = viewerError.next
      viewerError.next = null
      if (error) (this as unknown as { $emit: (e: string, v: unknown) => void }).$emit('error', error)
    },
    template: '<div data-test="pdf" :data-src="src" />',
  },
}))
vi.mock('@/components/common/BackButton.vue', () => ({
  default: { props: ['label'], template: '<button type="button" data-test="back">{{ label }}</button>' },
}))

import PDFViewerView from '@/views/PDFViewerView.vue'
import { PdfFetchError, PdfPasswordCancelled } from '@/utils/pdfjs'

afterEach(() => {
  vi.clearAllMocks()
  viewerError.next = null
  viewerError.mounts = 0
  Object.assign(query, { src: '/api/files/tickets/7/a.pdf', filename: 'invoice.pdf' })
})

describe('PDFViewerView', () => {
  it("names the tab after the PDF, and gives the title back when it's left", async () => {
    const wrapper = mountWithProviders(PDFViewerView)
    await flushPromises()
    expect(toValue(activeClaim.value?.claim.title)).toBe('invoice.pdf')

    wrapper.unmount()
    expect(activeClaim.value).toBeNull()
  })

  it('offers a way back', async () => {
    const wrapper = mountWithProviders(PDFViewerView)
    await flushPromises()
    expect(wrapper.find('[data-test="back"]').exists()).toBe(true)
    wrapper.unmount()
  })

  it('opens a file whose name has a "%" in it', async () => {
    Object.assign(query, { src: '/api/files/tickets/7/50%25%20off.pdf', filename: '50% off.pdf' })
    const wrapper = mountWithProviders(PDFViewerView)
    await flushPromises()
    expect(toValue(activeClaim.value?.claim.title)).toBe('50% off.pdf')
    expect(wrapper.find('[data-test="pdf"]').attributes('data-src')).toBe('/api/files/tickets/7/50%25%20off.pdf')
    wrapper.unmount()
  })

  it.each([
    [new PdfFetchError(404, 'HTTP 404'), 'pdf-viewer-not-found-title'],
    [new PdfFetchError(null, 'Load failed'), 'pdf-viewer-network-title'],
    [Object.assign(new Error('Invalid PDF structure.'), { name: 'InvalidPDFException' }), 'pdf-viewer-invalid-title'],
    [new Error('Something odd'), 'pdf-viewer-error-title'],
  ])('explains a failed load in its own words (%s)', async (error, title) => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    viewerError.next = error
    const wrapper = mountWithProviders(PDFViewerView)
    await flushPromises()
    const panel = wrapper.find('[data-testid="pdf-viewer-failure"]')
    expect(panel.find('h3').text()).toBe(title)
    expect(panel.text()).not.toContain((error as Error).message)
    wrapper.unmount()
  })

  it('offers the password again after the prompt is closed', async () => {
    viewerError.next = new PdfPasswordCancelled()
    const wrapper = mountWithProviders(PDFViewerView)
    await flushPromises()
    expect(wrapper.find('[data-testid="pdf-viewer-failure"] h3').text()).toBe('pdf-viewer-locked-title')

    const enter = wrapper.findAll('button').find((b) => b.text() === 'pdf-viewer-locked-enter-password')
    await enter!.trigger('click')
    await flushPromises()
    expect(viewerError.mounts).toBe(2)
    expect(wrapper.find('[data-test="pdf"]').exists()).toBe(true)
    wrapper.unmount()
  })

  it('says so when the link names no file', async () => {
    Object.assign(query, { src: undefined })
    const wrapper = mountWithProviders(PDFViewerView)
    await flushPromises()
    expect(wrapper.find('[data-testid="pdf-viewer-failure"]').text()).toContain('pdf-viewer-error-no-source')
    expect(wrapper.find('[data-test="pdf"]').exists()).toBe(false)
    wrapper.unmount()
  })
})
