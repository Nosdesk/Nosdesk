import { afterEach, describe, expect, it, vi } from 'vitest'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

vi.mock('@nosdesk/core/services/ticketService', () => ({ updateTicket: vi.fn() }))

import SpamBanner from '../SpamBanner.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

// Only admins may delete a ticket; the server refuses anyone else.
describe('SpamBanner', () => {
  it('offers an agent only Not spam', () => {
    wrapper = mountWithProviders(SpamBanner, { ticketId: 7 })
    expect(wrapper.text()).toContain('ticket-spam-not-spam')
    expect(wrapper.text()).not.toContain('ticket-spam-delete')
  })

  it('offers an admin Delete too', async () => {
    wrapper = mountWithProviders(SpamBanner, { ticketId: 7, canDelete: true })
    const del = wrapper.findAll('button').find((b) => b.text().includes('ticket-spam-delete'))
    expect(del).toBeDefined()
  })
})
