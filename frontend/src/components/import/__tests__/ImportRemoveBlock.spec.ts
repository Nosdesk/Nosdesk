import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { ImportJob, RemovalPlan } from '@nosdesk/core/services/importService'

const api = vi.hoisted(() => ({ removalPlan: vi.fn(), removeCreated: vi.fn(), get: vi.fn() }))
vi.mock('@nosdesk/core/services/importService', () => ({
  importService: {
    removalPlan: (...a: unknown[]) => api.removalPlan(...a),
    removeCreated: (...a: unknown[]) => api.removeCreated(...a),
    get: (...a: unknown[]) => api.get(...a),
  },
}))
vi.mock('@/components/common/ConfirmModal.vue', () => ({
  default: {
    props: ['show', 'title', 'message', 'confirmLabel', 'confirmDisabled', 'variant'],
    emits: ['confirm', 'close'],
    template: `<div v-if="show" data-test="confirm" :data-variant="variant">
      <p data-test="confirm-title">{{ title }}</p>
      <p data-test="confirm-message">{{ message }}</p>
      <slot name="body" />
      <button type="button" data-test="confirm-ok" :disabled="confirmDisabled" @click="$emit('confirm')">{{ confirmLabel }}</button>
    </div>`,
  },
}))

import ImportRemoveBlock from '@/components/import/ImportRemoveBlock.vue'

function job(overrides: Partial<ImportJob> = {}): ImportJob {
  return {
    id: 'job-1',
    job_type: 'assets',
    status: 'done',
    state: 'imported',
    pruned_at: null,
    filename: 'Cameras.xlsx',
    created_by: 'u-1',
    created_at: '2026-10-15T00:00:00Z',
    updated_at: '2026-10-15T00:00:00Z',
    completed_at: '2026-10-15T00:00:00Z',
    summary: {
      row_count: 412,
      would_create: 407,
      would_update: 0,
      unchanged: 0,
      errors: [],
      errors_truncated: false,
      errors_total: 5,
      rows_with_errors: 5,
    },
    records_committed: 407,
    error_message: null,
    source: null,
    suggested_mapping: [],
    mapping: null,
    options: null,
    removal_closes_at: '2999-11-14T00:00:00Z',
    ...overrides,
  }
}

const PLAN: RemovalPlan = {
  created: 407,
  deletable: 398,
  gone: 0,
  kept: [
    { id: 7, tag: 'TVS-0007', name: 'Cam 7', reasons: [{ code: 'ticket', ticket_id: 412 }] },
    { id: 8, tag: 'TVS-0008', name: 'Cam 8', reasons: [{ code: 'media', photos: 2, files: 0 }] },
  ],
  catalog: { manufacturers: 1, models: 9, groups: 0 },
  closes_at: '2999-11-14T00:00:00Z',
}

let wrapper: VueWrapper | null = null

function mount(found: ImportJob, webhooks = false) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:any(.*)*', component: { template: '<div />' } }],
  })
  wrapper = mountWithProviders(ImportRemoveBlock, { job: found, webhooks }, {}, [router])
  return wrapper
}

beforeEach(() => {
  for (const fn of Object.values(api)) fn.mockReset()
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('ImportRemoveBlock', () => {
  it('offers the ghost danger button with its window', () => {
    mount(job())
    const block = wrapper!.find('[data-test="import-remove"]')
    expect(block.attributes('data-state')).toBe('offer')
    const button = wrapper!.find('[data-test="import-remove-button"]')
    expect(button.attributes('data-variant')).toBe('ghost-danger')
    expect(button.text()).toBe('import-remove-button')
    expect(wrapper!.find('[data-test="import-remove-window"]').text()).toBe('import-remove-window')
  })

  it('reads the plan when pressed and asks with its counts, kept list and webhook line', async () => {
    api.removalPlan.mockResolvedValue(PLAN)
    mount(job(), true)
    expect(wrapper!.find('[data-test="confirm"]').exists()).toBe(false)
    await wrapper!.find('[data-test="import-remove-button"]').trigger('click')
    await flushPromises()
    expect(api.removalPlan).toHaveBeenCalledWith('job-1')
    const dialog = wrapper!.find('[data-test="confirm"]')
    expect(dialog.attributes('data-variant')).toBe('danger')
    expect(wrapper!.find('[data-test="confirm-title"]').text()).toBe('import-remove-title')
    expect(wrapper!.find('[data-test="confirm-message"]').text()).toBe(
      'import-remove-created import-remove-deletable import-remove-kept import-remove-catalog import-remove-webhooks',
    )
    expect(wrapper!.find('[data-test="import-remove-dialog-kept"]').text()).toContain('TVS-0007')
    expect(wrapper!.find('[data-test="confirm-ok"]').attributes('disabled')).toBeUndefined()
  })

  it('leaves the webhook line out when no webhook hears about assets', async () => {
    api.removalPlan.mockResolvedValue(PLAN)
    mount(job(), false)
    await wrapper!.find('[data-test="import-remove-button"]').trigger('click')
    await flushPromises()
    expect(wrapper!.find('[data-test="confirm-message"]').text()).not.toContain('import-remove-webhooks')
  })

  it("can't confirm when every asset has been changed since", async () => {
    api.removalPlan.mockResolvedValue({ ...PLAN, created: 2, deletable: 0, catalog: { manufacturers: 0, models: 0, groups: 0 } })
    mount(job())
    await wrapper!.find('[data-test="import-remove-button"]').trigger('click')
    await flushPromises()
    expect(wrapper!.find('[data-test="confirm-title"]').text()).toBe('import-remove-title-none')
    expect(wrapper!.find('[data-test="confirm-ok"]').attributes('disabled')).toBeDefined()
  })

  it('removes on confirm and hands the job with its result back', async () => {
    api.removalPlan.mockResolvedValue(PLAN)
    const removed = job({
      removed_at: '2026-10-15T00:00:00Z',
      removed_by_name: 'Priya Nair',
      remove_result: { created: 407, deleted: 398, gone: 0, deleted_ids: [], kept: PLAN.kept },
    })
    api.removeCreated.mockResolvedValue(removed)
    mount(job())
    await wrapper!.find('[data-test="import-remove-button"]').trigger('click')
    await flushPromises()
    await wrapper!.find('[data-test="confirm-ok"]').trigger('click')
    await flushPromises()
    expect(api.removeCreated).toHaveBeenCalledWith('job-1')
    const emitted = wrapper!.findComponent(ImportRemoveBlock).emitted('removed')
    expect(emitted?.[0]).toEqual([removed])
    expect(wrapper!.find('[data-test="confirm"]').exists()).toBe(false)
  })

  describe('when the removal fails', () => {
    const removed = () =>
      job({
        removed_at: '2026-10-15T00:00:00Z',
        removed_by_name: 'Sam Lee',
        remove_result: { created: 407, deleted: 398, gone: 0, deleted_ids: [], kept: [] },
      })

    async function confirmRemoval() {
      api.removalPlan.mockResolvedValue(PLAN)
      mount(job())
      await wrapper!.find('[data-test="import-remove-button"]').trigger('click')
      await flushPromises()
      await wrapper!.find('[data-test="confirm-ok"]').trigger('click')
      await flushPromises()
    }

    const block = () => wrapper!.findComponent(ImportRemoveBlock)

    afterEach(() => {
      vi.useRealTimers()
    })

    it('says another admin removed them already and shows the job as it is', async () => {
      api.removeCreated.mockRejectedValue({
        response: { status: 409, data: { code: 'import_already_removed', error: 'English' } },
      })
      api.get.mockResolvedValue(removed())
      await confirmRemoval()
      expect(api.get).toHaveBeenCalledWith('job-1')
      expect(block().emitted('refreshed')?.[0]).toEqual([removed()])
      expect(wrapper!.text()).toContain('import-remove-already-removed')
      expect(wrapper!.text()).not.toContain('import-remove-failed')
    })

    it('says the window closed when the check finds it closed, and shows the job as it is', async () => {
      api.removalPlan.mockRejectedValue({
        response: { status: 410, data: { code: 'import_removal_window_closed' } },
      })
      const closed = job({ pruned_at: '2026-11-15T00:00:00Z' })
      api.get.mockResolvedValue(closed)
      mount(job())
      await wrapper!.find('[data-test="import-remove-button"]').trigger('click')
      await flushPromises()
      expect(block().emitted('refreshed')?.[0]).toEqual([closed])
      expect(wrapper!.text()).toContain('import-remove-window-closed')
    })

    it('waits on a removal that timed out until the job says it ran', async () => {
      vi.useFakeTimers({ toFake: ['setTimeout'] })
      api.removeCreated.mockRejectedValue(new Error('timeout'))
      api.get.mockResolvedValueOnce(job()).mockResolvedValue(removed())
      await confirmRemoval()
      expect(wrapper!.find('[data-test="import-remove-waiting"]').text()).toBe('import-remove-still-running')
      await vi.advanceTimersByTimeAsync(2000)
      await flushPromises()
      expect(block().emitted('removed')).toBeUndefined()
      await vi.advanceTimersByTimeAsync(2000)
      await flushPromises()
      expect(block().emitted('removed')?.[0]).toEqual([removed()])
      expect(wrapper!.find('[data-test="import-remove-waiting"]').exists()).toBe(false)
      expect(wrapper!.text()).not.toContain('import-remove-failed')
    })

    it('reads the job back before calling a failure', async () => {
      api.removeCreated.mockRejectedValue({ response: { status: 500, data: {} } })
      api.get.mockResolvedValue(removed())
      await confirmRemoval()
      expect(block().emitted('removed')?.[0]).toEqual([removed()])
      expect(wrapper!.text()).not.toContain('import-remove-failed')
    })

    it('says it failed when the job shows it didn\'t run', async () => {
      api.removeCreated.mockRejectedValue({ response: { status: 500, data: {} } })
      api.get.mockResolvedValue(job())
      await confirmRemoval()
      expect(block().emitted('removed')).toBeUndefined()
      expect(wrapper!.text()).toContain('import-remove-failed')
    })
  })

  it('shows the result with the kept list and reasons in place of the button', () => {
    mount(
      job({
        removed_at: '2026-10-15T00:00:00Z',
        removed_by: 'u-1',
        removed_by_name: 'Priya Nair',
        remove_result: { created: 407, deleted: 398, gone: 0, deleted_ids: [], kept: PLAN.kept },
      }),
    )
    expect(wrapper!.find('[data-test="import-remove-button"]').exists()).toBe(false)
    const result = wrapper!.find('[data-test="import-remove-result"]')
    expect(result.text()).toContain('import-remove-result import-remove-result-kept')
    const kept = wrapper!.findAll('[data-test="import-remove-kept"] li')
    expect(kept).toHaveLength(2)
    expect(kept[0]!.text()).toContain('TVS-0007')
    expect(kept[0]!.text()).toContain('import-remove-reason-ticket')
    expect(kept[1]!.text()).toContain('import-remove-reason-photos')
    expect(kept[0]!.find('a').attributes('href')).toBe('/assets/7')
  })

  it('says the window closed after 30 days, and offers nothing', () => {
    mount(job({ removal_closes_at: '2020-11-14T00:00:00Z' }))
    expect(wrapper!.find('[data-test="import-remove-closed"]').text()).toBe('import-remove-window-closed')
    expect(wrapper!.find('[data-test="import-remove-button"]').exists()).toBe(false)
  })

  it('says the window closed for a pruned import too', () => {
    mount(job({ pruned_at: '2026-11-15T00:00:00Z' }))
    expect(wrapper!.find('[data-test="import-remove-closed"]').exists()).toBe(true)
  })

  it('renders nothing for a users import', () => {
    mount(job({ job_type: 'users', removal_closes_at: null }))
    expect(wrapper!.find('[data-test="import-remove"]').exists()).toBe(false)
  })
})
