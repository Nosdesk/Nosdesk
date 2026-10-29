import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref, shallowRef } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'

// The viewer hands pdf.js's PDFViewer the document and drives it from our
// toolbar and keys. pdf.js is replaced by a recording stand-in.

const fake = vi.hoisted(() => {
  class EventBus {
    handlers = new Map<string, Array<(event: Record<string, unknown>) => void>>()
    dispatched: Array<[string, Record<string, unknown>]> = []
    on(name: string, handler: (event: Record<string, unknown>) => void) {
      this.handlers.set(name, [...(this.handlers.get(name) ?? []), handler])
    }
    dispatch(name: string, event: Record<string, unknown> = {}) {
      this.dispatched.push([name, event])
      for (const handler of this.handlers.get(name) ?? []) handler(event)
    }
  }
  class PDFFindController {
    setDocument = vi.fn()
  }
  const state = {
    viewer: null as null | InstanceType<typeof PDFViewer>,
    linkOptions: null as null | Record<string, unknown>,
    eventBus: null as null | EventBus,
  }
  class PDFLinkService {
    constructor(options: Record<string, unknown>) {
      state.linkOptions = options
      state.eventBus = options.eventBus as EventBus
    }
    setViewer = vi.fn()
    setDocument = vi.fn()
  }
  class PDFViewer {
    options: Record<string, unknown>
    currentScaleValue = ''
    pagesRotation = 0
    #page = 1
    // Like pdf.js, a new page announces itself before the setter returns.
    get currentPageNumber() {
      return this.#page
    }
    set currentPageNumber(page: number) {
      if (page === this.#page) return
      this.#page = page
      state.eventBus?.dispatch('pagechanging', { pageNumber: page })
    }
    setDocument = vi.fn()
    increaseScale = vi.fn()
    decreaseScale = vi.fn()
    updateScale = vi.fn()
    nextPage = vi.fn(() => true)
    previousPage = vi.fn(() => true)
    constructor(options: Record<string, unknown>) {
      this.options = options
      state.viewer = this
    }
  }
  const lib = {
    EventBus,
    PDFLinkService,
    PDFViewer,
    PDFFindController,
    LinkTarget: { BLANK: 2 },
    FindState: { FOUND: 0, NOT_FOUND: 1, WRAPPED: 2, PENDING: 3 },
  }
  const pdfjs = { version: '6.4.299', AnnotationMode: { ENABLE: 1 }, AnnotationEditorType: { DISABLE: -1 } }
  return { state, lib, pdfjs }
})

vi.mock('@/utils/pdfjs', () => ({
  loadPdfjs: async () => fake.pdfjs,
  loadPdfjsViewer: async () => fake.lib,
  pdfjsImagesUrl: (version: string) => `/pdfjs/${version}/images/`,
}))

const loader = vi.hoisted(() => ({
  onPassword: null as null | ((reason: 'needed' | 'incorrect') => Promise<string | null>),
  load: null as null | ((src: string) => Promise<void>),
}))
const pdfDocument = shallowRef<unknown>(null)
const isLoading = ref(false)
const progress = shallowRef<{ loaded: number; total: number | null } | null>(null)
vi.mock('@/composables/usePdfDocument', () => ({
  usePdfDocument: (options: { onPassword: typeof loader.onPassword }) => {
    loader.onPassword = options.onPassword
    loader.load = vi.fn(async () => {})
    return { pdfDocument, isLoading, error: shallowRef(null), progress, load: loader.load, close: async () => {} }
  },
}))

const printer = vi.hoisted(() => ({
  print: vi.fn(async (_doc: unknown, options: { onProgress?: (done: number, total: number) => void }) => {
    options.onProgress?.(1, 5)
  }),
}))
vi.mock('@/utils/printPdf', () => ({ printPdfDocument: printer.print }))

const platform = vi.hoisted(() => ({
  tauri: false,
  openExternalUrl: vi.fn(async () => {}),
  shareFile: vi.fn(async (_bytes: Uint8Array, _name: string, _type: string) => ({ completed: true })),
}))
vi.mock('@/platform', () => ({
  isTauriRuntime: () => platform.tauri,
  openExternalUrl: platform.openExternalUrl,
  shareFile: platform.shareFile,
}))

import PDFViewer from '@/components/ticketComponents/PDFViewer.vue'
import { mountWithProviders } from '@/test/mountWithProviders'

let wrapper: VueWrapper | null = null
const events: string[] = []

async function mountViewer(props: Record<string, unknown> = {}) {
  wrapper = mountWithProviders(PDFViewer, {
    src: '/api/files/tickets/7/a.pdf',
    filename: 'a.pdf',
    onReady: () => events.push('ready'),
    onClose: () => events.push('close'),
    ...props,
  })
  await flushPromises()
  return fake.state.viewer!
}

// Give the viewer a document and let pdf.js lay it out.
async function openDocument(pages = 5) {
  pdfDocument.value = { numPages: pages }
  fake.state.eventBus!.dispatch('pagesinit')
  await flushPromises()
}

const scroller = () => wrapper!.find('[data-testid="pdf-scroller"]')
const press = (key: string, init: KeyboardEventInit = {}) =>
  scroller().element.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }))

beforeEach(() => {
  events.length = 0
  pdfDocument.value = null
  isLoading.value = false
  progress.value = null
  platform.tauri = false
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

describe('PDFViewer', () => {
  it('loads the file and sets pdf.js up read-only, with safe external links', async () => {
    const viewer = await mountViewer()
    expect(loader.load).toHaveBeenCalledWith('/api/files/tickets/7/a.pdf')
    expect(viewer.options).toMatchObject({ textLayerMode: 1, annotationMode: 1, annotationEditorMode: -1 })
    expect(viewer.options).not.toHaveProperty('scriptingManager')
    // Annotation icons from this pdf.js version's assets, labels from our catalogue.
    expect(viewer.options.imageResourcesPath).toBe('/pdfjs/6.4.299/images/')
    expect(viewer.options.l10n).toEqual(expect.objectContaining({ translate: expect.any(Function) }))
    expect(fake.state.linkOptions).toMatchObject({ externalLinkTarget: 2, externalLinkRel: 'noopener noreferrer nofollow' })
  })

  it('shows the document at automatic zoom, on the page asked for', async () => {
    const viewer = await mountViewer({ initialPage: 3 })
    await openDocument()
    expect(viewer.setDocument).toHaveBeenCalledWith({ numPages: 5 })
    expect(viewer.currentScaleValue).toBe('auto')
    expect(viewer.currentPageNumber).toBe(3)
    expect(events).toContain('ready')
  })

  it('keeps the page field and the announced page in step with scrolling', async () => {
    await mountViewer()
    await openDocument()
    fake.state.eventBus!.dispatch('pagechanging', { pageNumber: 4 })
    await flushPromises()
    expect((wrapper!.find('[data-testid="pdf-page-input"]').element as HTMLInputElement).value).toBe('4')
    expect(wrapper!.find('[data-testid="pdf-page-status"]').text()).toBe('ticket-media-pdf-page-status')
  })

  it('jumps to a typed page, within the document', async () => {
    const viewer = await mountViewer()
    await openDocument()
    const input = wrapper!.find('[data-testid="pdf-page-input"]')
    await input.setValue('99')
    await input.trigger('keydown', { key: 'Enter' })
    expect(viewer.currentPageNumber).toBe(5)
    expect((input.element as HTMLInputElement).value).toBe('5')
    await input.setValue('0')
    await input.trigger('keydown', { key: 'Enter' })
    expect(viewer.currentPageNumber).toBe(1)
    expect((input.element as HTMLInputElement).value).toBe('1')
  })

  it('shows the page it went to when the typed page was out of range', async () => {
    const viewer = await mountViewer()
    await openDocument()
    const input = wrapper!.find('[data-testid="pdf-page-input"]')
    viewer.currentPageNumber = 5
    await flushPromises()
    await input.setValue('999')
    await input.trigger('keydown', { key: 'Enter' })
    await flushPromises()
    expect(viewer.currentPageNumber).toBe(5)
    expect((input.element as HTMLInputElement).value).toBe('5')
  })

  it('zooms with + - 0 and turns pages with the side arrows', async () => {
    const viewer = await mountViewer()
    await openDocument()
    press('+')
    expect(viewer.increaseScale).toHaveBeenCalled()
    press('-')
    expect(viewer.decreaseScale).toHaveBeenCalled()
    viewer.currentScaleValue = '2'
    press('0')
    expect(viewer.currentScaleValue).toBe('auto')
    press('ArrowRight')
    expect(viewer.nextPage).toHaveBeenCalled()
    press('ArrowLeft')
    expect(viewer.previousPage).toHaveBeenCalled()
  })

  it('keeps the zoom keys after a choice in the zoom picker, which holds focus', async () => {
    // The desktop picker, a combobox that keeps focus once a zoom is picked.
    const narrow = window.matchMedia
    window.matchMedia = (query: string) => ({ ...narrow(query), matches: query.includes('min-width') })
    const viewer = await mountViewer().finally(() => (window.matchMedia = narrow))
    await openDocument()
    const picker = wrapper!.find('[role="combobox"]').element
    const pressOnPicker = (key: string) =>
      picker.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
    pressOnPicker('-')
    expect(viewer.decreaseScale).toHaveBeenCalled()
    viewer.currentScaleValue = '2'
    pressOnPicker('0')
    expect(viewer.currentScaleValue).toBe('auto')
    pressOnPicker('=')
    expect(viewer.increaseScale).toHaveBeenCalled()
    // The arrows still belong to the picker.
    pressOnPicker('ArrowRight')
    expect(viewer.nextPage).not.toHaveBeenCalled()
  })

  it('leaves the zoom keys to a text field', async () => {
    const viewer = await mountViewer()
    await openDocument()
    const input = wrapper!.find('[data-testid="pdf-page-input"]').element
    input.dispatchEvent(new KeyboardEvent('keydown', { key: '-', bubbles: true, cancelable: true }))
    expect(viewer.decreaseScale).not.toHaveBeenCalled()
  })

  it('zooms a step per wheel notch with Ctrl or Cmd held, and smoothly on a pinch', async () => {
    const viewer = await mountViewer()
    await openDocument()
    const wheel = (init: WheelEventInit) =>
      scroller().element.dispatchEvent(new WheelEvent('wheel', { cancelable: true, ...init }))

    // A pinch: ctrlKey set with no Ctrl key down, small pixel deltas.
    wheel({ deltaY: -2, ctrlKey: true })
    expect(viewer.updateScale).toHaveBeenLastCalledWith(expect.objectContaining({ scaleFactor: Math.exp(0.02) }))
    viewer.updateScale.mockClear()

    // A mouse with Ctrl held: one step per 100 px notch, halves add up.
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control', ctrlKey: true }))
    wheel({ deltaY: -2, ctrlKey: true })
    wheel({ deltaY: -50, ctrlKey: true })
    expect(viewer.updateScale).not.toHaveBeenCalled()
    wheel({ deltaY: -48, ctrlKey: true })
    expect(viewer.updateScale).toHaveBeenLastCalledWith(expect.objectContaining({ steps: 1 }))
    wheel({ deltaY: 300, ctrlKey: true })
    expect(viewer.updateScale).toHaveBeenLastCalledWith(expect.objectContaining({ steps: -3 }))
    window.dispatchEvent(new KeyboardEvent('keyup', { key: 'Control' }))

    // Firefox counts lines, three to a notch; Cmd is never a pinch.
    viewer.updateScale.mockClear()
    wheel({ deltaY: 3, deltaMode: WheelEvent.DOM_DELTA_LINE, metaKey: true })
    expect(viewer.updateScale).toHaveBeenLastCalledWith(expect.objectContaining({ steps: -1 }))
    viewer.updateScale.mockClear()
    wheel({ deltaY: -3, metaKey: true })
    expect(viewer.updateScale).not.toHaveBeenCalled()
  })

  it('zooms the document, not the app, on Ctrl or Cmd + wheel', async () => {
    const viewer = await mountViewer()
    await openDocument()
    const wheel = new WheelEvent('wheel', { deltaY: -100, ctrlKey: true, clientX: 10, clientY: 20, cancelable: true })
    scroller().element.dispatchEvent(wheel)
    expect(wheel.defaultPrevented).toBe(true)
    expect(viewer.updateScale).toHaveBeenCalledWith(expect.objectContaining({ steps: 1, origin: [10, 20] }))

    const plain = new WheelEvent('wheel', { deltaY: 100, cancelable: true })
    scroller().element.dispatchEvent(plain)
    expect(plain.defaultPrevented).toBe(false)
  })

  it('rotates a quarter turn at a time', async () => {
    const viewer = await mountViewer()
    await openDocument()
    viewer.pagesRotation = 270
    await wrapper!.find('button[aria-label="ticket-media-pdf-rotate"]').trigger('click')
    expect(viewer.pagesRotation).toBe(0)
  })

  it('asks to close on Escape', async () => {
    await mountViewer()
    press('Escape')
    expect(events).toContain('close')
  })

  it('lets go of the document when it closes', async () => {
    const viewer = await mountViewer()
    await openDocument()
    wrapper!.unmount()
    wrapper = null
    expect(viewer.setDocument).toHaveBeenLastCalledWith(null)
  })

  it('opens external links in the system browser in the app', async () => {
    platform.tauri = true
    await mountViewer()
    const layer = document.createElement('div')
    layer.className = 'annotationLayer'
    layer.innerHTML = '<a href="https://example.com/x" target="_blank">x</a><a href="#page=2">toc</a>'
    scroller().element.querySelector('.pdfViewer')!.appendChild(layer)

    const external = new MouseEvent('click', { bubbles: true, cancelable: true })
    layer.querySelector('a[target]')!.dispatchEvent(external)
    expect(external.defaultPrevented).toBe(true)
    expect(platform.openExternalUrl).toHaveBeenCalledWith('https://example.com/x')

    const internal = new MouseEvent('click', { bubbles: true, cancelable: true })
    layer.querySelector('a[href="#page=2"]')!.dispatchEvent(internal)
    expect(internal.defaultPrevented).toBe(false)
    expect(platform.openExternalUrl).toHaveBeenCalledTimes(1)
  })
})

describe('PDFViewer password', () => {
  it('asks for the password when pdf.js needs one, and passes it on', async () => {
    await mountViewer()
    expect(document.body.querySelector('[role="dialog"]')).toBeNull()

    const answer = loader.onPassword!('needed')
    await new Promise((r) => setTimeout(r, 20))
    const input = document.body.querySelector<HTMLInputElement>('[role="dialog"] input')!
    input.value = 'hunter2'
    input.dispatchEvent(new Event('input'))
    await flushPromises()
    input.form!.requestSubmit()
    await expect(answer).resolves.toBe('hunter2')
  })

  it('answers no password when the viewer closes with the prompt open', async () => {
    await mountViewer()
    const answer = loader.onPassword!('needed')
    wrapper!.unmount()
    wrapper = null
    await expect(answer).resolves.toBeNull()
  })
})

describe('PDFViewer find', () => {
  const dispatched = (name: string) => fake.state.eventBus!.dispatched.filter(([n]) => n === name).map(([, e]) => e)

  it('opens on Cmd or Ctrl+F, and searches as you type', async () => {
    await mountViewer()
    await openDocument()
    const keydown = new KeyboardEvent('keydown', { key: 'f', metaKey: true, bubbles: true, cancelable: true })
    scroller().element.dispatchEvent(keydown)
    expect(keydown.defaultPrevented).toBe(true)
    await flushPromises()

    const input = wrapper!.find('[data-testid="pdf-find-input"]')
    expect(document.activeElement).toBe(input.element)
    await input.setValue('invoice')
    expect(dispatched('find').at(-1)).toMatchObject({ query: 'invoice', type: '', caseSensitive: false, highlightAll: true })

    await input.trigger('keydown', { key: 'Enter', shiftKey: true })
    expect(dispatched('find').at(-1)).toMatchObject({ query: 'invoice', type: 'again', findPrevious: true })
  })

  it('shows how many matches pdf.js found', async () => {
    await mountViewer()
    await openDocument()
    press('f', { ctrlKey: true })
    await flushPromises()
    await wrapper!.find('[data-testid="pdf-find-input"]').setValue('total')
    fake.state.eventBus!.dispatch('updatefindcontrolstate', { state: 0, matchesCount: { current: 2, total: 7 } })
    await flushPromises()
    expect(wrapper!.find('[data-testid="pdf-find-status"]').text()).toBe('ticket-media-pdf-find-count')
    fake.state.eventBus!.dispatch('updatefindcontrolstate', { state: 1, matchesCount: { current: 0, total: 0 } })
    await flushPromises()
    expect(wrapper!.find('[data-testid="pdf-find-status"]').text()).toBe('ticket-media-pdf-find-none')
  })

  it('closes the find bar on Escape, and only the find bar', async () => {
    await mountViewer()
    await openDocument()
    press('f', { ctrlKey: true })
    await flushPromises()
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    const seen: string[] = []
    window.addEventListener('keydown', () => seen.push('window'), { once: true })
    wrapper!.find('[data-testid="pdf-find-input"]').element.dispatchEvent(escape)
    await flushPromises()
    expect(wrapper!.find('[data-testid="pdf-find-input"]').exists()).toBe(false)
    expect(dispatched('findbarclose')).toHaveLength(1)
    expect(events).not.toContain('close')
    // The overlay's dialog listens on window; the Escape never reaches it.
    expect(seen).toEqual([])
  })
})

describe('PDFViewer print and download', () => {
  it('prints the document from the toolbar or Cmd/Ctrl+P, and not in the app', async () => {
    await mountViewer()
    await openDocument()
    const keydown = new KeyboardEvent('keydown', { key: 'p', ctrlKey: true, bubbles: true, cancelable: true })
    scroller().element.dispatchEvent(keydown)
    expect(keydown.defaultPrevented).toBe(true)
    await flushPromises()
    expect(printer.print).toHaveBeenCalledWith({ numPages: 5 }, expect.objectContaining({ signal: expect.any(AbortSignal) }))
    wrapper!.unmount()

    platform.tauri = true
    await mountViewer()
    await openDocument()
    expect(wrapper!.find('button[aria-label="ticket-media-pdf-print"]').exists()).toBe(false)
  })

  it('shows how much of a large file has arrived', async () => {
    await mountViewer()
    isLoading.value = true
    progress.value = { loaded: 512, total: 2048 }
    await flushPromises()
    expect(wrapper!.find('[data-testid="pdf-download-status"]').text()).toBe('ticket-media-pdf-downloading-percent')
    progress.value = { loaded: 512, total: null }
    await flushPromises()
    expect(wrapper!.find('[data-testid="pdf-download-status"]').text()).toBe('ticket-media-pdf-downloading-size')
  })
})

describe('PDFViewer share in the app', () => {
  it('hands the document to the share sheet in place of a download link', async () => {
    platform.tauri = true
    await mountViewer({ filename: 'invoice.pdf' })
    const bytes = new Uint8Array([37, 80, 68, 70])
    pdfDocument.value = { numPages: 5, getData: async () => bytes }
    fake.state.eventBus!.dispatch('pagesinit')
    await flushPromises()

    expect(wrapper!.find('a[download]').exists()).toBe(false)
    await wrapper!.find('[data-testid="pdf-share"]').trigger('click')
    await flushPromises()
    expect(platform.shareFile).toHaveBeenCalledWith(bytes, 'invoice.pdf', 'application/pdf')
  })

  it('keeps the download link on the web', async () => {
    await mountViewer()
    await openDocument()
    expect(wrapper!.find('a[download]').exists()).toBe(true)
    expect(wrapper!.find('[data-testid="pdf-share"]').exists()).toBe(false)
  })
})
