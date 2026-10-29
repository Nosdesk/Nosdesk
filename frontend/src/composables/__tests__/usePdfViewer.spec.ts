import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'

// A document that finishes loading after the viewer has gone (or after a
// newer file was asked for) is released instead of kept.

const pending = vi.hoisted(() => ({ resolvers: [] as Array<(doc: unknown) => void> }))
vi.mock('@/utils/pdfjs', () => ({
  loadPdfjs: async () => ({}),
  fetchPdfBytes: async () => new Uint8Array(),
  openPdfDocument: () => ({}),
  awaitPdfDocument: () =>
    new Promise((resolve) => {
      pending.resolvers.push(resolve)
    }),
}))

import { usePdfViewer } from '@/composables/usePdfViewer'

function fakeDocument() {
  const destroy = vi.fn(async () => {})
  const page = { getViewport: () => ({ width: 600, height: 800 }) }
  return { numPages: 1, getPage: async () => page, loadingTask: { destroy } }
}

function mountViewer(onReady = vi.fn()) {
  let viewer!: ReturnType<typeof usePdfViewer>
  const wrapper = mount(
    defineComponent({
      setup() {
        viewer = usePdfViewer({ onReady })
        return () => h('div')
      },
    }),
  )
  return { wrapper, viewer: () => viewer, onReady }
}

afterEach(() => {
  pending.resolvers = []
})

describe('usePdfViewer', () => {
  it('releases a document that arrives after the viewer closed', async () => {
    const { wrapper, viewer, onReady } = mountViewer()
    void viewer().loadDocument('/a.pdf')
    await flushPromises()

    wrapper.unmount()
    const late = fakeDocument()
    pending.resolvers[0]!(late)
    await flushPromises()

    expect(late.loadingTask.destroy).toHaveBeenCalled()
    expect(onReady).not.toHaveBeenCalled()
  })

  it('shows the newest file when an older one finishes last', async () => {
    const { wrapper, viewer, onReady } = mountViewer()
    void viewer().loadDocument('/old.pdf')
    await flushPromises()
    void viewer().loadDocument('/new.pdf')
    await flushPromises()

    const newer = fakeDocument()
    pending.resolvers[1]!(newer)
    await flushPromises()
    const older = fakeDocument()
    pending.resolvers[0]!(older)
    await flushPromises()

    expect(older.loadingTask.destroy).toHaveBeenCalled()
    expect(newer.loadingTask.destroy).not.toHaveBeenCalled()
    expect(onReady).toHaveBeenCalledTimes(1)
    expect(viewer().totalPages.value).toBe(1)

    wrapper.unmount()
    await flushPromises()
    expect(newer.loadingTask.destroy).toHaveBeenCalled()
  })
})
