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
vi.mock('@/components/ticketComponents/PDFViewer.vue', () => ({
  default: { props: ['src', 'filename'], template: '<div data-test="pdf" :data-src="src" />' },
}))
vi.mock('@/components/common/BackButton.vue', () => ({
  default: { props: ['label'], template: '<button type="button" data-test="back">{{ label }}</button>' },
}))

import PDFViewerView from '@/views/PDFViewerView.vue'

afterEach(() => {
  vi.clearAllMocks()
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
})
