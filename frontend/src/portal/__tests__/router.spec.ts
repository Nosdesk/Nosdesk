import { describe, expect, it, vi } from 'vitest'

// The views aren't under test; the router only needs something to mount.
vi.mock('../views/LoginView.vue', () => ({ default: { template: '<div />' } }))
vi.mock('../views/NewTicketView.vue', () => ({ default: { template: '<div />' } }))
vi.mock('../views/TicketsView.vue', () => ({ default: { template: '<div />' } }))
vi.mock('../views/TicketView.vue', () => ({ default: { template: '<div />' } }))
vi.mock('../embed', () => ({ isEmbed: false }))

import router from '../router'

describe('portal router', () => {
  it('sends the short new-request link to the new request form', async () => {
    await router.push('/new')
    expect(router.currentRoute.value.name).toBe('ticket-new')
  })
})
