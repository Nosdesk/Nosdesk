import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import type { AssetMedia } from '@nosdesk/core/types/asset'

// Asset media holds photos and documents. Photos render as thumbnails;
// documents (a service report PDF here) render as a file row with a download
// link, never as an image that would show broken.

const PHOTO: AssetMedia = {
  id: 1,
  asset_id: 7,
  url: '/api/files/assets/7/media/a_label.jpg',
  thumbnail_url: '/api/files/assets/7/media/thumb/a.webp',
  name: 'label.jpg',
  file_size: 2048,
  mime_type: 'image/jpeg',
  kind: 'photo',
  sort_order: 0,
  created_at: '2026-10-01T00:00:00Z',
}
const REPORT: AssetMedia = {
  id: 2,
  asset_id: 7,
  url: '/api/files/assets/7/media/b_service-report.pdf',
  thumbnail_url: null,
  name: 'service-report.pdf',
  file_size: 1536,
  mime_type: 'application/pdf',
  kind: 'document',
  sort_order: 0,
  created_at: '2026-10-02T00:00:00Z',
}

vi.mock('@nosdesk/core/services/assetMediaService', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@nosdesk/core/services/assetMediaService')>()
  return { ...actual, assetMediaService: { list: async () => [PHOTO, REPORT] } }
})
vi.mock('@/composables/useSyncActions', () => ({ useSyncActions: () => {} }))

import AssetMediaPanel from '@/components/assets/AssetMediaPanel.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('AssetMediaPanel', () => {
  it('shows photos as thumbnails and documents as downloadable file rows', async () => {
    wrapper = mount(AssetMediaPanel, {
      props: { assetId: 7, canEdit: true },
      global: {
        plugins: [
          createFluentVue({ bundles: [new FluentBundle('en-US')] }),
          createPinia(),
          [PiniaColada, {}] as never,
        ],
        stubs: { Modal: true },
      },
    })
    await flushPromises()

    const images = wrapper.findAll('img')
    expect(images).toHaveLength(1)
    expect(images[0].attributes('src')).toBe(PHOTO.thumbnail_url)

    const downloads = wrapper.findAll(`a[download="${REPORT.name}"]`)
    expect(downloads.length).toBeGreaterThan(0)
    expect(downloads[0].attributes('href')).toBe(REPORT.url)
    expect(wrapper.text()).toContain('service-report.pdf')
    expect(wrapper.text()).toContain('1.5 KB')
  })
})
