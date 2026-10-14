import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const collections = vi.hoisted(() => ({ setCollectionVisibility: vi.fn(async () => true) }))
vi.mock('@nosdesk/core/services/collectionService', () => collections)
vi.mock('@nosdesk/core/services/groupService', () => ({
  groupService: { getGroups: async () => [{ id: 3, name: 'Payroll' }] },
}))
vi.mock('@/components/common/AssignmentPicker.vue', () => ({
  default: { name: 'AssignmentPicker', props: ['selectedItems'], template: '<div data-testid="picker" />' },
}))
vi.mock('@/components/Modal.vue', () => ({
  default: { template: '<div><slot /><slot name="footer" /></div>' },
}))

import CollectionVisibilityModal from '@/components/documentationComponents/CollectionVisibilityModal.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

describe('CollectionVisibilityModal', () => {
  it('shows a restricted collection with nobody listed as admins only', async () => {
    wrapper = mountWithProviders(CollectionVisibilityModal, {
      collectionId: 7,
      currentGroupIds: [],
      currentUsers: [],
      restricted: true,
    })
    await flushPromises()

    expect(wrapper.find('[data-testid="doc-access-admins-only"]').exists()).toBe(true)
    expect(wrapper.text()).not.toContain('docs-collection-visibility-public')
    const chosen = wrapper.findAll('[role="radio"]').find((b) => b.text().includes('docs-access-chosen'))
    expect(chosen?.attributes('aria-checked')).toBe('true')
  })

  it('keeps it restricted to nobody when saved', async () => {
    wrapper = mountWithProviders(CollectionVisibilityModal, {
      collectionId: 7,
      currentGroupIds: [3],
      currentUsers: [],
      restricted: true,
    })
    await flushPromises()
    const picker = wrapper.findComponent({ name: 'AssignmentPicker' })
    picker.vm.$emit('update:selectedItems', [])
    await flushPromises()
    const save = wrapper.findAll('button').find((b) => b.text().includes('docs-collection-visibility-save'))
    await save!.trigger('click')
    await flushPromises()

    expect(collections.setCollectionVisibility).toHaveBeenCalledWith(7, [], [], true)
  })
})
