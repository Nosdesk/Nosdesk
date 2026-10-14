import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada, useQueryCache } from '@pinia/colada'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import type { AssetLoan } from '@nosdesk/core/types/asset'

// The asset's loan panel keeps showing the loans it has when a refetch fails
// (as one does while the network is down), and recovers once it is back.

const LOAN: AssetLoan = {
  id: 1,
  asset_id: 7,
  borrower_user_uuid: 'u-1',
  loaned_at: '2026-10-01T00:00:00Z',
  returned_at: '2026-10-05T00:00:00Z',
}
const api = vi.hoisted(() => ({ fail: false, calls: 0 }))
vi.mock('@nosdesk/core/services/assetLoanService', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@nosdesk/core/services/assetLoanService')>()
  return {
    ...actual,
    assetLoanService: {
      list: async () => {
        api.calls++
        if (api.fail) throw new Error('Network Error')
        return [LOAN]
      },
    },
  }
})
vi.mock('@/composables/useSyncActions', () => ({ useSyncActions: () => {} }))
vi.mock('@/composables/useUsersDirectory', () => ({
  useUsersDirectory: () => ({ getUserHandle: () => ({ user: { value: { name: 'Ada' } } }) }),
}))
vi.mock('@/composables/useTicketNumberLookup', () => ({ numberForTicketId: () => undefined }))

import AssetLoanPanel from '@/components/assets/AssetLoanPanel.vue'

const LOAD_ERROR = 'asset-loan-load-error'
let wrapper: VueWrapper | null = null
beforeEach(() => {
  api.fail = false
  api.calls = 0
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

async function mountPanel() {
  let cache!: ReturnType<typeof useQueryCache>
  wrapper = mount(AssetLoanPanel, {
    props: { assetId: 7, currentStatus: 'in_service' },
    shallow: true,
    global: {
      plugins: [
        createFluentVue({ bundles: [new FluentBundle('en-US')] }),
        createPinia(),
        [PiniaColada, {}] as never,
        { install: (app) => void app.runWithContext(() => (cache = useQueryCache())) },
      ],
    },
  })
  await flushPromises()
  return cache
}

describe('AssetLoanPanel', () => {
  it('keeps its loans and hides the error when a refetch fails, and recovers on reconnect', async () => {
    const cache = await mountPanel()
    expect(wrapper!.text()).not.toContain(LOAD_ERROR)
    expect(wrapper!.text()).toContain('asset-loan-range')

    api.fail = true
    await cache.invalidateQueries({ key: ['asset-loans'] }).catch(() => {})
    await flushPromises()
    expect(api.calls).toBe(2)
    expect(wrapper!.text()).not.toContain(LOAD_ERROR)
    expect(wrapper!.text()).toContain('asset-loan-range')

    api.fail = false
    window.dispatchEvent(new Event('online'))
    await flushPromises()
    expect(api.calls).toBe(3)
    expect(cache.getEntries({ key: ['asset-loans'] })[0].state.value.error).toBeNull()
  })

  it('says it could not load the loans when it has none to show, until a fetch works', async () => {
    api.fail = true
    await mountPanel()
    expect(wrapper!.text()).toContain(LOAD_ERROR)

    api.fail = false
    window.dispatchEvent(new Event('online'))
    await flushPromises()
    expect(wrapper!.text()).not.toContain(LOAD_ERROR)
  })
})
