import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'

// A document that finishes loading after the viewer has gone (or after a
// newer file was asked for) is released instead of kept.

const pending = vi.hoisted(() => ({ resolvers: [] as Array<(doc: unknown) => void>, reject: null as unknown }))
vi.mock('@/utils/pdfjs', () => ({
  loadPdfjs: async () => ({}),
  fetchPdfBytes: async () => new Uint8Array(),
  openPdfDocument: () => ({}),
  awaitPdfDocument: () => {
    if (pending.reject) {
      const error = pending.reject
      pending.reject = null
      return Promise.reject(error)
    }
    return new Promise((resolve) => {
      pending.resolvers.push(resolve)
    })
  },
}))

import { usePdfDocument } from '@/composables/usePdfDocument'

function fakeDocument() {
  return { numPages: 1, loadingTask: { destroy: vi.fn(async () => {}) } }
}

function mountLoader() {
  let loader!: ReturnType<typeof usePdfDocument>
  const wrapper = mount(
    defineComponent({
      setup() {
        loader = usePdfDocument()
        return () => h('div')
      },
    }),
  )
  return { wrapper, loader: () => loader }
}

afterEach(() => {
  pending.resolvers = []
})

describe('usePdfDocument', () => {
  it('releases a document that arrives after the viewer closed', async () => {
    const { wrapper, loader } = mountLoader()
    void loader().load('/a.pdf')
    await flushPromises()

    wrapper.unmount()
    const late = fakeDocument()
    pending.resolvers[0]!(late)
    await flushPromises()

    expect(late.loadingTask.destroy).toHaveBeenCalled()
    expect(loader().pdfDocument.value).toBeNull()
  })

  it('shows the newest file when an older one finishes last', async () => {
    const { wrapper, loader } = mountLoader()
    void loader().load('/old.pdf')
    await flushPromises()
    void loader().load('/new.pdf')
    await flushPromises()

    const newer = fakeDocument()
    pending.resolvers[1]!(newer)
    await flushPromises()
    const older = fakeDocument()
    pending.resolvers[0]!(older)
    await flushPromises()

    expect(older.loadingTask.destroy).toHaveBeenCalled()
    expect(newer.loadingTask.destroy).not.toHaveBeenCalled()
    expect(loader().pdfDocument.value).toBe(newer)
    expect(loader().isLoading.value).toBe(false)

    wrapper.unmount()
    await flushPromises()
    expect(newer.loadingTask.destroy).toHaveBeenCalled()
  })

  it('reports a failed load, and clears the report on the next one', async () => {
    const { wrapper, loader } = mountLoader()
    pending.reject = new Error('Invalid PDF structure.')
    await loader().load('/bad.pdf')
    expect((loader().error.value as Error).message).toBe('Invalid PDF structure.')

    void loader().load('/good.pdf')
    expect(loader().error.value).toBeNull()
    wrapper.unmount()
  })
})
