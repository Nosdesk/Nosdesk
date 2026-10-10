import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const collections = vi.hoisted(() => ({
  createCollection: vi.fn(),
  updateCollection: vi.fn(async () => ({ id: 7 })),
  setCollectionVisibility: vi.fn(async () => true),
}))
vi.mock('@nosdesk/core/services/collectionService', () => collections)
vi.mock('@nosdesk/core/sync/stores/documentation', () => ({
  useSyncDocsStore: () => ({ allCollections: [] }),
}))
vi.mock('@/components/Modal.vue', () => ({
  default: { template: '<div><slot /><slot name="footer" /></div>' },
}))
vi.mock('@/components/common/AssignmentPicker.vue', () => ({
  default: { name: 'AssignmentPicker', props: ['selectedItems'], template: '<div />' },
}))
vi.mock('@/components/common/ColorHueSlider.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/DocumentIconSelector.vue', () => ({ default: { template: '<div />' } }))

import CollectionModal from '@/components/documentationComponents/CollectionModal.vue'

const COLLECTION = {
  id: 7,
  uuid: 'c-7',
  name: 'Payroll',
  slug: 'payroll',
  description: '',
  icon: '📁',
  color: '#6366f1',
  is_system: false,
  description_doc_id: 'ws-1_collection-c-7',
  visible_to_groups: [{ id: 3, name: 'Finance' }],
  visible_to_users: [],
  is_public: false,
  restricted: true,
  page_count: 0,
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

describe('CollectionModal (edit)', () => {
  it('leaves who can open it alone when that did not change', async () => {
    wrapper = mountWithProviders(CollectionModal, { mode: 'edit', show: true, collection: COLLECTION })
    await flushPromises()
    const save = wrapper.findAll('button').find((b) => b.text().includes('docs-edit-collection-save'))
    expect(save, 'the save button').toBeTruthy()
    await save!.trigger('click')
    await flushPromises()

    expect(collections.updateCollection).toHaveBeenCalledTimes(1)
    expect(collections.setCollectionVisibility).not.toHaveBeenCalled()
  })

  it('saves who can open it when that changed', async () => {
    wrapper = mountWithProviders(CollectionModal, { mode: 'edit', show: true, collection: COLLECTION })
    await flushPromises()
    wrapper.findComponent({ name: 'AssignmentPicker' }).vm.$emit('update:selectedItems', [])
    await flushPromises()
    const save = wrapper.findAll('button').find((b) => b.text().includes('docs-edit-collection-save'))
    await save!.trigger('click')
    await flushPromises()

    expect(collections.setCollectionVisibility).toHaveBeenCalledWith(7, [], [], true)
  })
})
