import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada, useQueryCache } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { AssetLoan } from '@nosdesk/core/types/asset'

// A loans card keeps showing the loans it has when a refetch fails (as one
// does while the network is down), says it couldn't load them only when it
// has none to show, and recovers once the network is back.

const LOAN: AssetLoan = {
  id: 1,
  asset_id: 7,
  borrower_user_uuid: 'u-1',
  loaned_at: '2026-10-01T00:00:00Z',
  ticket_id: 42,
}
const api = vi.hoisted(() => ({ fail: false, calls: 0 }))
vi.mock('@nosdesk/core/services/assetLoanService', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@nosdesk/core/services/assetLoanService')>()
  return {
    ...actual,
    assetLoanService: {
      listByTicket: async () => {
        api.calls++
        if (api.fail) throw new Error('Network Error')
        return [LOAN]
      },
    },
  }
})
vi.mock('@/composables/useSyncActions', () => ({ useSyncActions: () => {} }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ isTechnician: true }) }))
// A row per loan; the rows' own rendering isn't under test here.
vi.mock('@/components/ticketComponents/TicketLoanRow.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      props: { loan: { type: Object, required: true } },
      setup: (props) => () => h('div', { 'data-loan': (props.loan as AssetLoan).id }),
    }),
  }
})
vi.mock('@/components/ticketComponents/IssueLoanerModal.vue', async () => {
  const { defineComponent } = await import('vue')
  return { default: defineComponent({ render: () => null }) }
})

import TicketLoansCard from '@/components/ticketComponents/TicketLoansCard.vue'

let wrapper: VueWrapper | null = null
beforeEach(() => {
  api.fail = false
  api.calls = 0
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

const LOAD_ERROR = 'asset-loan-load-error'
const rows = () => document.body.querySelectorAll('[data-loan]')

async function mountCard() {
  let cache!: ReturnType<typeof useQueryCache>
  wrapper = mountWithProviders(TicketLoansCard, { ticketId: 42, hasDevices: true }, {}, [
    [PiniaColada, {}] as never,
    { install: (app) => void app.runWithContext(() => (cache = useQueryCache())) },
  ])
  await flushPromises()
  return cache
}

describe('TicketLoansCard', () => {
  it('keeps its loans and hides the error when a refetch fails, and recovers on reconnect', async () => {
    const cache = await mountCard()
    expect(document.body.textContent).toContain('asset-loan-ticket-heading')
    expect(document.body.textContent).not.toContain(LOAD_ERROR)
    const rowsBefore = rows().length
    expect(rowsBefore).toBe(1)

    // The network drops and the loans are refetched.
    api.fail = true
    await cache.invalidateQueries({ key: ['ticket-loans'] }).catch(() => {})
    await flushPromises()
    expect(api.calls).toBe(2)
    expect(document.body.textContent).not.toContain(LOAD_ERROR)
    expect(rows().length).toBe(1)

    // Back online: Pinia Colada refetches the failed query, and its error
    // clears.
    api.fail = false
    window.dispatchEvent(new Event('online'))
    await flushPromises()
    expect(api.calls).toBe(3)
    expect(cache.getEntries({ key: ['ticket-loans'] })[0].state.value.error).toBeNull()
    expect(document.body.textContent).not.toContain(LOAD_ERROR)
  })

  it('says it could not load the loans when it has none to show, until a fetch works', async () => {
    api.fail = true
    await mountCard()
    expect(document.body.textContent).toContain(LOAD_ERROR)

    api.fail = false
    window.dispatchEvent(new Event('online'))
    await flushPromises()
    expect(document.body.textContent).not.toContain(LOAD_ERROR)
    expect(rows().length).toBe(1)
  })
})
