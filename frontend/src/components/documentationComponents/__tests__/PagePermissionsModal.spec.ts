import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const docs = vi.hoisted(() => ({
  getPageVisibility: vi.fn(),
  setPageVisibility: vi.fn(async () => true),
}))
vi.mock('@nosdesk/core/services/documentationService', () => docs)
vi.mock('@nosdesk/core/services/collectionService', () => ({
  getCollectionsForPage: async () => [],
}))
vi.mock('@/components/common/AssignmentPicker.vue', () => ({
  default: { name: 'AssignmentPicker', props: ['selectedItems'], template: '<div />' },
}))
vi.mock('@/components/Modal.vue', () => ({
  default: { template: '<div><slot /><slot name="footer" /></div>' },
}))

import PagePermissionsModal from '@/components/documentationComponents/PagePermissionsModal.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

const save = (w: VueWrapper) =>
  w.findAll('button').find((b) => b.text().includes('docs-page-permissions-save'))!

describe('PagePermissionsModal', () => {
  it('shows whether guests can read the page and saves a change with its rules', async () => {
    docs.getPageVisibility.mockResolvedValue({ restricted: false, is_public: true, groups: [], users: [] })
    wrapper = mountWithProviders(PagePermissionsModal, { pageId: 5 })
    await flushPromises()

    const toggle = wrapper.find('[role="switch"]')
    expect(toggle.attributes('aria-checked')).toBe('true')
    expect(save(wrapper).attributes('disabled')).toBeDefined()

    await toggle.trigger('click')
    await save(wrapper).trigger('click')
    await flushPromises()

    expect(docs.setPageVisibility).toHaveBeenCalledWith(5, [], [], false, false)
  })
})
