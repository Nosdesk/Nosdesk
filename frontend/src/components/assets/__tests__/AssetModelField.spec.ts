import { describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { flushPromises } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// An asset imported with a model name the catalog doesn't have keeps that
// name as text. The Model field shows it and offers to link it, and a new
// catalog model starts from it.

vi.mock('@/composables/useAssetCatalogQuery', () => ({
  useAssetModelsQuery: () => ({
    models: ref([
      { id: 1, manufacturer_id: 10, name: 'Latitude 5540', kind: 'laptop' },
      { id: 2, manufacturer_id: 20, name: 'MacBook Air 13', kind: 'laptop' },
    ]),
  }),
  useManufacturersQuery: () => ({
    manufacturers: ref([
      { id: 10, name: 'Dell' },
      { id: 20, name: 'Apple' },
    ]),
  }),
}))
vi.mock('@/composables/useAssetKindsQuery', () => ({
  useAssetKindsQuery: () => ({ kinds: ref([{ slug: 'laptop', label: 'Laptop' }]) }),
}))
vi.mock('@/services/assetService', () => ({ setAssetModel: vi.fn(), clearAssetModel: vi.fn() }))
vi.mock('@/components/Modal.vue', () => ({
  default: { props: ['show'], template: '<div v-if="show" data-test="modal"><slot /></div>' },
}))
vi.mock('@/components/common/SearchableDropdown.vue', () => ({
  default: {
    props: ['modelValue', 'options'],
    emits: ['update:model-value'],
    template: '<div data-test="dropdown" />',
  },
}))

import AssetModelField from '@/components/assets/AssetModelField.vue'

const base = { assetId: 11, kind: 'laptop', editable: true }

describe('AssetModelField', () => {
  it('shows model text that has no catalog link, with a way to link it', () => {
    const w = mountWithProviders(AssetModelField,
      { ...base, modelId: null, manufacturerSnapshot: null, modelSnapshot: 'MacBook Air 13' },
    )
    expect(w.text()).toContain('MacBook Air 13')
    expect(w.text()).toContain('asset-model-link')
    expect(w.text()).not.toContain('asset-model-choose')
  })

  it('offers to choose a model when there is no model text', () => {
    const w = mountWithProviders(AssetModelField,
      { ...base, modelId: null, manufacturerSnapshot: null, modelSnapshot: null },
    )
    expect(w.text()).toContain('asset-model-choose')
  })

  it('shows the text read-only to someone who cannot edit', () => {
    const w = mountWithProviders(AssetModelField,
      { ...base, editable: false, modelId: null, manufacturerSnapshot: 'Apple', modelSnapshot: 'MacBook Air 13' },
    )
    expect(w.text()).toContain('Apple · MacBook Air 13')
    expect(w.text()).not.toContain('asset-model-link')
  })

  it('lists the catalog model with the same name first, and starts a new model from the text', async () => {
    const w = mountWithProviders(AssetModelField,
      { ...base, modelId: null, manufacturerSnapshot: 'Apple', modelSnapshot: 'macbook air 13' },
    )
    await w.findAll('button').find((b) => b.text() === 'asset-model-link')!.trigger('click')
    const dropdown = w.findComponent('[data-test="dropdown"]')
    const options = dropdown.props('options') as { value: string }[]
    expect(options.map((o) => o.value)).toEqual(['__create__', '2', '1'])

    dropdown.vm.$emit('update:model-value', '__create__')
    await flushPromises()
    const inputs = w.find('[data-test="modal"]').findAll('input')
    expect(inputs.map((i) => (i.element as HTMLInputElement).value)).toContain('macbook air 13')
  })
})
