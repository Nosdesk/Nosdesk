import { describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import FirstTicketsState from '@/components/views/FirstTicketsState.vue'

function mountState(isAdmin: boolean, onCreate?: () => void) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/admin/email', name: 'admin-email-settings', component: { template: '<div />' } },
      { path: '/:pathMatch(.*)*', component: { template: '<div />' } },
    ],
  })
  return mountWithProviders(FirstTicketsState, { isAdmin, onCreate }, {}, [router])
}

// The test bundle carries no catalogue, so keys render as themselves.
describe('FirstTicketsState', () => {
  it('shows an admin the three ways a ticket arrives, and the invite', () => {
    const w = mountState(true)
    const text = w.text()
    expect(text).toContain('ticket-list-first-title')
    expect(text).toContain('ticket-list-first-connect-mailbox')
    expect(text).toContain('ticket-list-first-share-form')
    expect(text).toContain('ticket-list-first-create')
    expect(text).toContain('ticket-list-first-invite')
    expect(w.find('a[href="/admin/email"]').exists()).toBe(true)
    expect(w.find('a[href="/users"]').exists()).toBe(true)
    w.unmount()
  })

  it('shows an agent only the heading and create', () => {
    const w = mountState(false)
    const text = w.text()
    expect(text).toContain('ticket-list-first-title')
    expect(text).toContain('ticket-list-first-create')
    expect(text).not.toContain('ticket-list-first-connect-mailbox')
    expect(text).not.toContain('ticket-list-first-share-form')
    expect(text).not.toContain('ticket-list-first-invite')
    w.unmount()
  })

  it('create asks the page action, not a route', async () => {
    let created = 0
    const w = mountState(false, () => created++)
    const btn = w.findAll('button').find((b) => b.text() === 'ticket-list-first-create')!
    await btn.trigger('click')
    expect(created).toBe(1)
    w.unmount()
  })
})
