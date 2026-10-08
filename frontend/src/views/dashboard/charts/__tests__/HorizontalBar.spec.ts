import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { computed } from 'vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { BreakdownBucket } from '@/services/analyticsService'

const breakdown = vi.hoisted(() => vi.fn())
vi.mock('@/services/analyticsService', () => ({ analyticsService: { breakdown } }))
vi.mock('@/composables/useTimeRange', () => ({
  useTimeRange: () => ({
    window: computed(() => ({ from: '2026-10-01T00:00:00Z', to: '2026-10-08T00:00:00Z' })),
  }),
}))

import HorizontalBar from '@/views/dashboard/charts/HorizontalBar.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

async function rows(groupBy: 'category' | 'assignee', buckets: BreakdownBucket[]): Promise<string> {
  breakdown.mockResolvedValue({ buckets })
  wrapper = mountWithProviders(HorizontalBar, { groupBy }, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  return wrapper.text()
}

describe('HorizontalBar', () => {
  it('names a category row by the category, not its id', async () => {
    const text = await rows('category', [
      { key: '7', label: 'Hardware', value: 2 },
      { key: 'none', label: null, value: 1 },
    ])
    expect(text).toContain('Hardware')
    expect(text).not.toContain('7')
    expect(text).toContain('dashboard-bar-uncategorised')
  })

  it('names an assignee row by the person, not their uuid', async () => {
    const uuid = '019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb'
    const text = await rows('assignee', [
      { key: uuid, label: 'Dana Agent', value: 2 },
      { key: 'unassigned', label: null, value: 1 },
    ])
    expect(text).toContain('Dana Agent')
    expect(text).not.toContain(uuid)
    expect(text).toContain('dashboard-bar-unassigned')
  })
})
