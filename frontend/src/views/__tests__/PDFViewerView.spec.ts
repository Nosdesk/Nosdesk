import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const setCustomTitle = vi.hoisted(() => vi.fn())
vi.mock('vue-router', () => ({
  useRoute: () => ({ query: { src: '%2Fapi%2Ffiles%2Ftickets%2F7%2Fa.pdf', filename: 'invoice.pdf' } }),
  useRouter: () => ({ push: vi.fn(), back: vi.fn() }),
}))
vi.mock('@/router/navigation', () => ({ performBack: vi.fn() }))
vi.mock('@/composables/useTitleManager', () => ({ useTitleManager: () => ({ setCustomTitle }) }))
vi.mock('@/components/ticketComponents/PDFViewer.vue', () => ({ default: { template: '<div />' } }))

import PDFViewerView from '@/views/PDFViewerView.vue'

afterEach(() => vi.clearAllMocks())

describe('PDFViewerView', () => {
  it("names the tab after the PDF, and gives the title back when it's left", async () => {
    const wrapper = mountWithProviders(PDFViewerView)
    await flushPromises()
    expect(setCustomTitle).toHaveBeenLastCalledWith('invoice.pdf')

    wrapper.unmount()
    expect(setCustomTitle).toHaveBeenLastCalledWith(null)
  })
})
