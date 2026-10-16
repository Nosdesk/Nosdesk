import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { ImportField, ImportValues } from '@nosdesk/core/services/importService'

const api = vi.hoisted(() => ({ previewValues: vi.fn() }))
vi.mock('@nosdesk/core/services/importService', () => ({
  importValuesPreviewQueryKey: (id: string, at: string, key: string) => ['import-values-preview', id, at, key],
  importService: { previewValues: (...a: unknown[]) => api.previewValues(...a) },
}))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ user: { uuid: 'me' } }) }))
vi.mock('@/components/common/SearchableDropdown.vue', () => ({
  default: {
    props: ['modelValue', 'options', 'ariaLabel'],
    emits: ['update:modelValue'],
    template: `<select :aria-label="ariaLabel" :value="modelValue" @change="$emit('update:modelValue', $event.target.value)">
      <option value=""></option>
      <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
    </select>`,
  },
}))

import ImportValuesStep from '@/components/import/ImportValuesStep.vue'

const FIELDS: ImportField[] = [
  { id: 'manufacturer', label: 'Manufacturer', group: 'core', value_type: 'text', required: false },
  { id: 'model', label: 'Model', group: 'core', value_type: 'text', required: false },
]

/** A Make card with an unmatched "Blackmagic" and a Model card under it. */
function view(): ImportValues {
  return {
    rows_checked: 3,
    rows_skipped: 0,
    skipped_other: [],
    columns: [
      {
        field: 'manufacturer',
        index: 0,
        header: 'Make',
        value_type: 'text',
        required: false,
        lookup: 'manufacturer',
        can_create: true,
        choices: [{ value: '1', label: 'Sony' }],
        distinct: 1,
        matched: 0,
        values: [
          {
            key: 'blackmagic',
            text: 'Blackmagic',
            blank: false,
            rows: [2, 3],
            match: 'no_match',
            suggested: { action: 'keep_text' },
            skipped_rows: [],
          },
        ],
      },
      {
        field: 'model',
        index: 1,
        header: 'Model',
        value_type: 'text',
        required: false,
        lookup: 'model',
        context: 'blackmagic',
        context_label: 'Blackmagic',
        can_create: true,
        choices: [],
        distinct: 1,
        matched: 0,
        values: [
          {
            key: 'ursa',
            text: 'URSA',
            blank: false,
            rows: [2],
            match: 'no_match',
            suggested: { action: 'keep_text' },
            skipped_rows: [],
          },
        ],
      },
    ],
  }
}

let wrapper: VueWrapper | null = null

function mount() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:any(.*)*', component: { template: '<div />' } }],
  })
  wrapper = mountWithProviders(
    ImportValuesStep,
    {
      view: view(),
      fields: FIELDS,
      checking: false,
      jobId: 'job-1',
      version: '2026-10-15T00:00:00Z',
      jobType: 'assets',
    },
    {},
    [[PiniaColada, {}] as never, router],
  )
  return wrapper
}

/** Answer Blackmagic Create and wait out the settle before the read. */
async function createMaker() {
  const row = wrapper!.findAll('[data-test="import-value-row"]').find((r) => r.attributes('data-value-id')?.startsWith('manufacturer:'))!
  await row.find('select').setValue('create')
  await new Promise((resolve) => setTimeout(resolve, 350))
  await flushPromises()
}

const modelCard = () =>
  wrapper!.findAll('[data-test="import-values-card"]').find((c) => c.attributes('data-field') === 'model')!

beforeEach(() => {
  api.previewValues.mockReset()
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('ImportValuesStep Model card under manufacturer answers', () => {
  it('says the card follows the answers once they are read', async () => {
    api.previewValues.mockResolvedValue(view())
    mount()
    await createMaker()
    expect(api.previewValues).toHaveBeenCalledTimes(1)
    expect(modelCard().attributes('data-stale')).toBeUndefined()
    expect(modelCard().find('[data-test="import-values-preview-failed"]').exists()).toBe(false)
    expect(modelCard().find('[data-test="import-values-card-note"]').text()).toBe('import-values-recomputed')
  })

  it('marks the card stale with a short line when the read fails, and reads again on Try again', async () => {
    api.previewValues.mockRejectedValue(new Error('offline'))
    mount()
    await createMaker()
    const card = modelCard()
    expect(card.attributes('data-stale')).toBe('true')
    expect(card.find('[data-test="import-values-preview-failed"]').text()).toContain('import-values-preview-failed')
    expect(card.find('[data-test="import-values-card-note"]').exists()).toBe(false)

    api.previewValues.mockResolvedValue(view())
    await card.find('[data-test="import-values-preview-retry"]').trigger('click')
    await flushPromises()
    expect(api.previewValues).toHaveBeenCalledTimes(2)
    expect(modelCard().attributes('data-stale')).toBeUndefined()
    expect(modelCard().find('[data-test="import-values-preview-failed"]').exists()).toBe(false)
  })
})
