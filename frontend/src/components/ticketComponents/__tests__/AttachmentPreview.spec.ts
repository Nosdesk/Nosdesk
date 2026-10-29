import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { configureAssetUrl, proxiedAssetResolver } from '@nosdesk/core/transport'
import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'

const pdfjsUtils = vi.hoisted(() => ({
  loadPdfjs: vi.fn(async () => ({})),
  fetchPdfBytes: vi.fn(async () => new Uint8Array()),
  openPdfDocument: vi.fn(),
}))
vi.mock('@/utils/pdfjs', () => pdfjsUtils)

import AttachmentPreview from '../AttachmentPreview.vue'

// jsdom never loads images, so the server-thumbnail probe would wait forever.
// `serverThumbnail` decides whether the probe finds one.
let serverThumbnail = false
class ProbeImage {
  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  set src(_: string) {
    setTimeout(() => (serverThumbnail ? this.onload?.() : this.onerror?.()))
  }
}
beforeEach(() => {
  serverThumbnail = false
  vi.stubGlobal('Image', ProbeImage)
})

afterEach(() => {
  configureAssetUrl((p) => p, (u) => u)
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

const stored = (name: string) => `/uploads/tickets/7/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb_${name}`
const served = (name: string) => `/api/files/tickets/7/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb_${name}`

/** An attachment as the comment thread renders it: a compact tile for every
 * non-voice file, or the full card (`compact` false) the composer shows. */
function tile(name: string, compact = true) {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:rest(.*)*', component: { render: () => null } }],
  })
  const wrapper = mount(AttachmentPreview, {
    props: {
      attachment: { id: 1, url: stored(name), name },
      author: 'Agent',
      timestamp: 'now',
      compact,
      showDelete: true,
    },
    global: {
      plugins: [createFluentVue({ bundles: [bundle] }), router],
      stubs: { Modal: true, Icon: true, Spinner: true, AssetImg: true },
    },
  })
  return Object.assign(wrapper, { router })
}

describe('AttachmentPreview (compact)', () => {
  it('downloads a file from its authenticated URL, under its name', () => {
    const link = tile('notes.txt').find('a')
    expect(link.exists()).toBe(true)
    expect(link.attributes('href')).toBe(served('notes.txt'))
    expect(link.attributes('download')).toBe('notes.txt')
    expect(link.attributes('aria-label')).toBe('Download notes.txt')
    expect(link.classes()).toContain('focus-visible:ring-2')
  })

  // The href resolves through the mobile asset scheme like every other file
  // link; a tap doesn't save a file in the app anywhere yet (pre-existing).
  it('resolves the link through the mobile app asset scheme there', () => {
    configureAssetUrl(proxiedAssetResolver('nosdesk-asset://localhost'))
    expect(tile('notes.txt').find('a').attributes('href')).toBe(`nosdesk-asset://localhost${served('notes.txt')}`)
  })

  it('downloads a video, which has no viewer in the thread', () => {
    const link = tile('clip.mp4').find('a')
    expect(link.attributes('href')).toBe(served('clip.mp4'))
    expect(link.attributes('download')).toBe('clip.mp4')
  })

  it('names the kind of file it deletes', () => {
    const label = (name: string) => tile(name).find('button[aria-label^="Delete"]').attributes('aria-label')
    expect(label('notes.txt')).toBe('Delete file')
    expect(label('clip.mp4')).toBe('Delete video')
    expect(label('memo.mp3')).toBe('Delete audio')
    expect(label('invoice.pdf')).toBe('Delete PDF')
    expect(label('photo.png')).toBe('Delete image')
    // Shown when the tile has keyboard focus, not only on hover.
    expect(tile('notes.txt').find('button[aria-label^="Delete"]').classes()).toContain('group-focus-within:opacity-100')
  })

  it('still opens an image in the preview and a PDF in the viewer', async () => {
    const image = tile('photo.png')
    expect(image.find('a').exists()).toBe(false)
    await image.find('button[aria-label="Open photo.png"]').trigger('click')
    expect(image.findComponent({ name: 'Modal' }).props('show')).toBe(true)

    const pdf = tile('invoice.pdf')
    expect(pdf.find('a').exists()).toBe(false)
    await pdf.find('button[aria-label="Open invoice.pdf"]').trigger('click')
    await pdf.router.isReady()
    await new Promise((r) => setTimeout(r))
    // Over the page it was opened from, by its stored path.
    expect(pdf.router.currentRoute.value.query.pdf).toBe(stored('invoice.pdf'))
  })

  // The router encodes the query once and decodes it once, so the overlay
  // reads the stored path as it was, a "%" in the name included.
  it('opens a PDF whose name has a "%" by its exact stored path', async () => {
    const pdf = tile('50% off.pdf')
    await pdf.find('button[aria-label="Open 50% off.pdf"]').trigger('click')
    await new Promise((r) => setTimeout(r))
    expect(pdf.router.currentRoute.value.query.pdf).toBe(stored('50% off.pdf'))
  })

  it('opens from the keyboard: the open target is a button named after the file', () => {
    const open = tile('invoice.pdf').find('button[aria-label="Open invoice.pdf"]')
    expect(open.attributes('type')).toBe('button')
    expect(open.classes()).toContain('focus-visible:ring-2')
  })

  it("doesn't download or render a PDF for a tile", async () => {
    tile('invoice.pdf')
    await new Promise((r) => setTimeout(r, 10))
    expect(pdfjsUtils.fetchPdfBytes).not.toHaveBeenCalled()
    expect(pdfjsUtils.loadPdfjs).not.toHaveBeenCalled()
  })

  it("shows the server's thumbnail on a PDF tile when there is one", async () => {
    serverThumbnail = true
    const pdf = tile('invoice.pdf')
    await new Promise((r) => setTimeout(r, 10))
    const thumb = pdf.findAllComponents({ name: 'AssetImg' })
    expect(thumb).toHaveLength(1)
    expect(thumb[0]!.attributes('src')).toBe(served('invoice_thumb.webp'))
  })
})

describe('AttachmentPreview (card)', () => {
  it('renders a PDF thumbnail without a server one, and releases the document', async () => {
    const destroy = vi.fn(async () => {})
    const page = {
      getViewport: ({ scale }: { scale: number }) => ({ width: 612 * scale, height: 792 * scale }),
      render: () => ({ promise: Promise.resolve() }),
    }
    pdfjsUtils.openPdfDocument.mockReturnValue({ promise: Promise.resolve({ getPage: async () => page }), destroy })
    vi.spyOn(HTMLCanvasElement.prototype, 'toDataURL').mockReturnValue('data:image/webp;base64,AAAA')

    const card = tile('invoice.pdf', false)
    await vi.waitFor(() => expect(destroy).toHaveBeenCalled())
    expect(card.findComponent({ name: 'AssetImg' }).attributes('src')).toBe('data:image/webp;base64,AAAA')
    expect(card.find('button[aria-label="Open invoice.pdf"]').exists()).toBe(true)
  })
})
