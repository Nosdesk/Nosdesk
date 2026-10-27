import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'

// The PDF viewer offers its fullscreen button in a ticket and not on the
// fullscreen page itself, with and without the workspace slug in the path.

let routing: 'host' | 'path' = 'host'
let slug: string | null = null
vi.mock('@nosdesk/core/services/instanceConfig', () => ({
  fetchInstanceConfig: async () => {},
  getWorkspaceRouting: () => routing,
}))
vi.mock('@/stores/workspaceReset', () => ({
  resetWorkspaceScopedState: async () => {},
  enterWorkspace: async () => {},
}))
vi.mock('@/services/activeWorkspace', () => ({
  beginWorkspaceSwitch: () => {},
  activeWorkspaceSlug: () => slug,
  setActiveWorkspaceSlug: (s: string | null) => {
    slug = s
  },
}))
vi.mock('@/composables/usePdfViewer', () => ({
  usePdfViewer: () => ({
    isLoading: ref(false),
    totalPages: ref(0),
    scale: ref(1),
    renderedPages: ref(new Map()),
    loadDocument: async () => {},
    renderPage: async () => {},
    getPageDimensions: () => null,
    getScaledMaxWidth: () => 0,
    zoomIn: () => {},
    zoomOut: () => {},
    fitToWidth: () => {},
  }),
}))

import PDFViewer from '@/components/ticketComponents/PDFViewer.vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { makeWorkspaceRouter, routingModes, stubRoute } from '@/test/workspaceRouter'

beforeAll(() => {
  if (typeof globalThis.IntersectionObserver !== 'function') {
    class StubIntersectionObserver {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    Object.defineProperty(globalThis, 'IntersectionObserver', {
      value: StubIntersectionObserver,
      configurable: true,
    })
  }
})

const routes = [stubRoute('/tickets/:id', 'ticket-view'), stubRoute('/pdf-viewer', 'pdf-viewer')]

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const fullscreenButton = () => wrapper!.find('button[aria-label="ticket-media-pdf-fullscreen-aria"]')

describe.each(routingModes)('PDFViewer ($mode routing)', ({ mode, prefix }) => {
  beforeEach(() => {
    routing = mode
    slug = null
  })

  it('offers fullscreen in a ticket and opens the viewer inside the workspace', async () => {
    const router = makeWorkspaceRouter(routes)
    await router.push(`${prefix}/tickets/12`)
    wrapper = mountWithProviders(PDFViewer, { src: '/api/files/a.pdf', filename: 'a.pdf' }, {}, [router])
    await flushPromises()
    expect(fullscreenButton().exists()).toBe(true)

    await fullscreenButton().trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('pdf-viewer')
    expect(router.currentRoute.value.path).toBe(`${prefix}/pdf-viewer`)
    expect(fullscreenButton().exists()).toBe(false)
  })

  it('has no fullscreen button on the fullscreen page', async () => {
    const router = makeWorkspaceRouter(routes)
    await router.push(`${prefix}/pdf-viewer?src=%2Fapi%2Ffiles%2Fa.pdf&filename=a.pdf`)
    wrapper = mountWithProviders(PDFViewer, { src: '/api/files/a.pdf', filename: 'a.pdf' }, {}, [router])
    await flushPromises()
    expect(fullscreenButton().exists()).toBe(false)
  })
})
