import { afterEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { configureAssetUrl, proxiedAssetResolver } from '@nosdesk/core/transport'
import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'

import AttachmentPreview from '../AttachmentPreview.vue'

afterEach(() => configureAssetUrl((p) => p, (u) => u))

const stored = (name: string) => `/uploads/tickets/7/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb_${name}`
const served = (name: string) => `/api/files/tickets/7/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb_${name}`

/** A compact reply tile, as the comment thread renders every non-voice file. */
function tile(name: string) {
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
      compact: true,
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
  })

  it('downloads through the mobile app asset scheme there', () => {
    configureAssetUrl(proxiedAssetResolver('nosdesk-asset://localhost'))
    expect(tile('notes.txt').find('a').attributes('href')).toBe(`nosdesk-asset://localhost${served('notes.txt')}`)
  })

  it('downloads a video, which has no viewer in the thread', () => {
    const link = tile('clip.mp4').find('a')
    expect(link.attributes('href')).toBe(served('clip.mp4'))
    expect(link.attributes('download')).toBe('clip.mp4')
  })

  it('names the kind of file it deletes', () => {
    const label = (name: string) => tile(name).find('button').attributes('aria-label')
    expect(label('notes.txt')).toBe('Delete file')
    expect(label('clip.mp4')).toBe('Delete video')
    expect(label('memo.mp3')).toBe('Delete audio')
    expect(label('invoice.pdf')).toBe('Delete PDF')
    expect(label('photo.png')).toBe('Delete image')
  })

  it('still opens an image in the preview and a PDF in the viewer', async () => {
    const image = tile('photo.png')
    expect(image.find('a').exists()).toBe(false)
    // The tile is the first root; the preview modal is the second.
    await image.find('div').trigger('click')
    expect(image.findComponent({ name: 'Modal' }).props('show')).toBe(true)

    const pdf = tile('invoice.pdf')
    expect(pdf.find('a').exists()).toBe(false)
    await pdf.find('div').trigger('click')
    await pdf.router.isReady()
    await new Promise((r) => setTimeout(r))
    expect(pdf.router.currentRoute.value.path).toBe('/pdf-viewer')
  })
})
