import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import type { AssetLifecycleEvent, AssetLoan } from '@nosdesk/core/types/asset'

// The asset page's state header names whoever has the asset and offers the
// one loan action that fits: Return while a loan is open, Loan out while the
// asset can be lent, neither for anyone who can't edit.

const api = vi.hoisted(() => ({
  loans: [] as AssetLoan[],
  events: [] as AssetLifecycleEvent[],
}))
vi.mock('@nosdesk/core/services/assetLoanService', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@nosdesk/core/services/assetLoanService')>()
  return { ...actual, assetLoanService: { list: async () => api.loans } }
})
vi.mock('@nosdesk/core/services/assetLifecycleService', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@nosdesk/core/services/assetLifecycleService')>()
  return { ...actual, assetLifecycleService: { list: async () => api.events } }
})
vi.mock('@/composables/useSyncActions', () => ({ useSyncActions: () => {} }))
vi.mock('@/composables/useUsersDirectory', () => ({
  useUsersDirectory: () => ({ getUserHandle: () => ({ user: { value: { name: 'Ada' } } }) }),
}))

import AssetStateHeader from '@/components/assets/AssetStateHeader.vue'

const OPEN_LOAN: AssetLoan = {
  id: 1,
  asset_id: 7,
  borrower_user_uuid: 'u-1',
  loaned_at: '2026-10-01T00:00:00Z',
  returned_at: null,
  due_back: '2020-01-01',
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  api.loans = []
  api.events = []
})

async function mountHeader(props: { status: string; canEdit?: boolean }) {
  wrapper = mount(AssetStateHeader, {
    props: { assetId: 7, ...props },
    shallow: true,
    global: {
      plugins: [
        createFluentVue({ bundles: [new FluentBundle('en-US')] }),
        createPinia(),
        [PiniaColada, {}] as never,
      ],
      stubs: {
        Button: { template: '<button><slot /></button>' },
        RouterLink: { template: '<a><slot /></a>' },
        StatusPill: { props: ['label'], template: '<span>{{ label }}</span>' },
      },
    },
  })
  await flushPromises()
  return wrapper
}

const buttons = (w: VueWrapper) => w.findAll('button').map((b) => b.text())

describe('AssetStateHeader', () => {
  it('names the borrower, flags an overdue loan and offers Return', async () => {
    api.loans = [OPEN_LOAN]
    const w = await mountHeader({ status: 'on_loan', canEdit: true })
    expect(w.text()).toContain('Ada')
    expect(w.text()).toContain('asset-loan-due-overdue')
    expect(buttons(w)).toEqual(['asset-loan-return'])
  })

  it('offers Loan out and Change status for an asset in service', async () => {
    const w = await mountHeader({ status: 'in_service', canEdit: true })
    expect(buttons(w)).toEqual(['asset-loan-loan-out', 'asset-lifecycle-change-status'])
  })

  it('offers only Change status where the asset cannot be lent', async () => {
    const w = await mountHeader({ status: 'in_repair', canEdit: true })
    expect(buttons(w)).toEqual(['asset-lifecycle-change-status'])
  })

  it('still names the borrower of an asset marked lost while out', async () => {
    api.loans = [OPEN_LOAN]
    const w = await mountHeader({ status: 'lost', canEdit: true })
    expect(w.text()).toContain('Ada')
    expect(buttons(w)).toEqual(['asset-loan-return', 'asset-lifecycle-change-status'])
  })

  it('says since when the asset has had its status', async () => {
    api.events = [
      {
        id: 3,
        asset_id: 7,
        from_status: 'in_service',
        to_status: 'in_repair',
        occurred_at: '2026-10-02T09:00:00Z',
      } as AssetLifecycleEvent,
    ]
    const w = await mountHeader({ status: 'in_repair' })
    expect(w.text()).toContain('asset-state-since')
  })

  it('shows the state without actions to someone who cannot edit', async () => {
    api.loans = [OPEN_LOAN]
    const w = await mountHeader({ status: 'on_loan' })
    expect(w.text()).toContain('Ada')
    expect(buttons(w)).toEqual([])
  })
})
