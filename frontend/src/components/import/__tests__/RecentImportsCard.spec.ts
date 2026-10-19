import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { ImportJobListItem } from '@nosdesk/core/services/importService'

const api = vi.hoisted(() => ({ list: vi.fn() }))
vi.mock('@nosdesk/core/services/importService', () => ({
  importJobsQueryKey: (limit: number) => ['import-jobs', limit],
  importService: { list: (...a: unknown[]) => api.list(...a) },
}))

import RecentImportsCard from '../RecentImportsCard.vue'

function item(overrides: Partial<ImportJobListItem>): ImportJobListItem {
  return {
    id: 'job-1',
    job_type: 'assets',
    status: 'done',
    state: 'imported',
    filename: 'Inventory.xlsx',
    sheet: 'Cameras',
    created_by: 'u-1',
    created_by_name: 'Priya Nair',
    created_at: '2026-10-15T00:00:00Z',
    updated_at: '2026-10-15T00:00:00Z',
    completed_at: '2026-10-15T00:00:00Z',
    pruned_at: null,
    counts: { new: 407, updated: 0, unchanged: 0, skipped: 5 },
    ...overrides,
  }
}

let wrapper: VueWrapper | null = null

async function mountCard() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/admin/data-import/documentation', name: 'admin-docs-import', component: { template: '<div />' } },
      { path: '/:any(.*)*', component: { template: '<div />' } },
    ],
  })
  await router.push('/admin/data-import')
  await router.isReady()
  wrapper = mountWithProviders(RecentImportsCard, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
  return wrapper
}

beforeEach(() => api.list.mockReset())
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('RecentImportsCard', () => {
  it('lists each job with its state pill, counts and action', async () => {
    api.list.mockResolvedValue([
      item({ id: 'draft-1', status: 'dry_run_done', state: 'draft', counts: { new: 1, updated: 0, unchanged: 0, skipped: 0 } }),
      item({ id: 'done-1' }),
      item({ id: 'old-1', status: 'parsed', state: 'expired', counts: null }),
    ])
    await mountCard()
    expect(api.list).toHaveBeenCalledWith(20)

    const rows = wrapper!.findAll('[data-test="recent-import"]')
    expect(rows.map((r) => r.attributes('data-state'))).toEqual(['draft', 'imported', 'expired'])
    expect(rows.map((r) => r.find('[data-test="recent-import-state"]').text())).toEqual([
      'import-history-state-draft',
      'import-history-state-imported',
      'import-history-state-expired',
    ])
    expect(rows.map((r) => r.find('[data-test="recent-import-counts"]').text())).toEqual([
      'import-history-draft-on-review',
      'import-history-count-new · import-history-count-skipped',
      'import-history-state-expired',
    ])

    expect(rows[0].find('[data-test="recent-import-resume"]').attributes('href')).toBe(
      '/admin/data-import/csv?job=draft-1',
    )
    expect(rows[1].find('[data-test="recent-import-view"]').attributes('href')).toBe(
      '/admin/data-import/imports/done-1',
    )
    expect(rows[2].find('a').exists()).toBe(false)
    expect(rows[2].text()).toContain('import-history-cannot-resume')
    expect(rows[1].text()).toContain('Inventory.xlsx · Cameras')
    expect(rows[1].text()).toContain('Priya Nair')
  })

  it('lists a documentation import, resuming its draft in its own wizard', async () => {
    api.list.mockResolvedValue([
      item({ id: 'docs-1', job_type: 'documentation', filename: 'IT-Handbook.zip', sheet: null, status: 'checking', state: 'draft', counts: null }),
      item({ id: 'docs-2', job_type: 'documentation', filename: 'IT-Handbook.zip', sheet: null, counts: { new: 36, updated: 0, unchanged: 0, skipped: 3 } }),
    ])
    await mountCard()
    const rows = wrapper!.findAll('[data-test="recent-import"]')
    expect(rows.map((r) => r.find('[data-test="recent-import-counts"]').text())).toEqual([
      'import-history-draft-on-pages',
      'import-history-count-new · import-history-count-skipped',
    ])
    expect(rows[0].find('[data-test="recent-import-resume"]').attributes('href')).toBe(
      '/admin/data-import/documentation?job=docs-1',
    )
    expect(rows[1].find('[data-test="recent-import-view"]').attributes('href')).toBe(
      '/admin/data-import/imports/docs-2',
    )
  })

  it('says so when nothing has been imported', async () => {
    api.list.mockResolvedValue([])
    await mountCard()
    expect(wrapper!.find('[data-test="recent-imports-empty"]').text()).toBe('import-history-empty')
  })
})
