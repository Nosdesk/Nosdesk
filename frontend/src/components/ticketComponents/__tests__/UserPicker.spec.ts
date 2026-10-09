import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const USERS = [
  { uuid: 'u-1', name: 'Ada Lovelace', email: 'ada@example.test', platform_role: 'user', workspace_role: 'agent', avatar_thumb: null, avatar_url: null },
  { uuid: 'u-2', name: 'Grace Hopper', email: 'grace@example.test', platform_role: 'user', workspace_role: 'agent', avatar_thumb: null, avatar_url: null },
  { uuid: 'u-3', name: 'Linus Torvalds', email: 'linus@example.test', platform_role: 'user', workspace_role: 'agent', avatar_thumb: null, avatar_url: null },
]
// The auth store drags the router (and every view) in; the picker
// reads two fields from it.
const auth = vi.hoisted(() => ({ user: null as Record<string, unknown> | null, isTechnician: false }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => auth }))
vi.mock('@/services/userService', () => ({
  default: {
    getPaginatedUsers: async ({ search }: { search?: string }) => {
      const q = (search ?? '').toLowerCase()
      const data = USERS.filter((u) => !q || u.name.toLowerCase().includes(q))
      return { data, total: data.length, page: 1, page_size: 50, total_pages: 1 }
    },
  },
}))

import UserPicker from '@/components/ticketComponents/UserPicker.vue'

let wrapper: VueWrapper | null = null
// Desktop: the anchored popup. The sheet path shares the combobox.
beforeEach(() => {
  window.innerWidth = 1024
})
afterEach(() => {
  auth.user = null
  auth.isTechnician = false
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

const settle = (ms = 0) => new Promise((r) => setTimeout(r, ms))
const options = () => Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
const groups = () =>
  Array.from(document.body.querySelectorAll('[role="listbox"] [role="group"]')).map((g) =>
    document.getElementById(g.getAttribute('aria-labelledby')!)?.textContent?.trim(),
  )
async function key(el: Element, key: string) {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  await nextTick()
}

async function mountPicker(props: Record<string, unknown> = {}) {
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }] })
  await router.push('/')
  wrapper = mountWithProviders(
    UserPicker,
    { modelValue: 'u-2', type: 'assignee', currentUser: { uuid: 'u-2', name: 'Grace Hopper', email: 'grace@example.test' }, ...props },
    {},
    [router],
  )
  await nextTick()
  return wrapper.get('input[role="combobox"]').element as HTMLInputElement
}

async function open(input: HTMLInputElement) {
  input.focus()
  input.dispatchEvent(new Event('focus'))
  await flushPromises()
  await settle()
  await nextTick()
}

describe('UserPicker', () => {
  it('lists you once when the ticket is already yours', async () => {
    auth.user = { uuid: 'u-2', name: 'Grace Hopper', email: 'grace@example.test', avatar_thumb: null, avatar_url: null }
    auth.isTechnician = true
    const input = await mountPicker()
    await open(input)
    expect(groups()).toEqual(['ticket-picker-user-section-selected-assignee', 'ticket-picker-user-section-staff'])
    expect(options().filter((o) => o.textContent?.includes('Grace Hopper'))).toHaveLength(1)
  })

  it('offers you beside the selection when the ticket is someone else\'s', async () => {
    auth.user = { uuid: 'u-1', name: 'Ada Lovelace', email: 'ada@example.test', avatar_thumb: null, avatar_url: null }
    auth.isTechnician = true
    const input = await mountPicker()
    await open(input)
    expect(groups()).toContain('ticket-picker-user-section-you')
    expect(options().filter((o) => o.textContent?.includes('Ada Lovelace'))).toHaveLength(1)
  })

  it('shows the current name closed and opens a grouped listbox highlighting it', async () => {
    const input = await mountPicker()
    expect(input.value).toBe('Grace Hopper')
    expect(input.getAttribute('aria-expanded')).toBe('false')
    await open(input)
    expect(input.getAttribute('aria-expanded')).toBe('true')
    expect(input.value).toBe('')
    expect(groups()).toEqual(['ticket-picker-user-section-selected-assignee', 'ticket-picker-user-section-staff'])
    // The selected user heads the list and is left out of the staff group.
    expect(options().map((o) => o.textContent?.trim().slice(0, 2))).toEqual(['GH', 'AL', 'LT'])
    expect(options().map((o) => o.getAttribute('aria-selected'))).toEqual(['true', 'false', 'false'])
    expect(options()[0].hasAttribute('data-highlighted')).toBe(true)
    expect(input.getAttribute('aria-activedescendant')).toBe(options()[0].id)
    expect(document.activeElement).toBe(input)
  })

  it('shows the name when it resolves after the uuid', async () => {
    // The seed lands late (a preview pane resolving the assignee).
    const input = await mountPicker({ currentUser: undefined })
    expect(input.value).toBe('u-2')
    await wrapper!.setProps({ currentUser: { uuid: 'u-2', name: 'Grace Hopper', email: 'grace@example.test' } })
    await nextTick()
    expect(input.value).toBe('Grace Hopper')
  })

  it('filters as typed and picks with Enter, then shows the new name', async () => {
    const updates: string[] = []
    const input = await mountPicker({ 'onUpdate:modelValue': (v: string) => updates.push(v) })
    await open(input)
    input.value = 'lin'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    await flushPromises()
    await settle(250)
    await flushPromises()
    expect(groups()).toEqual(['ticket-picker-user-section-results'])
    expect(options().map((o) => o.textContent?.trim().slice(0, 2))).toEqual(['LT'])
    await key(input, 'Enter')
    expect(updates).toEqual(['u-3'])
    await settle(5)
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()
    expect(input.getAttribute('aria-expanded')).toBe('false')
  })

  it('closes on Escape with the name restored, and clears from the inline button', async () => {
    const updates: string[] = []
    const input = await mountPicker({ 'onUpdate:modelValue': (v: string) => updates.push(v) })
    await open(input)
    await key(input, 'Escape')
    await settle(5)
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()
    expect(input.value).toBe('Grace Hopper')
    await wrapper!.get('button[aria-label="ticket-picker-user-clear"]').trigger('click')
    expect(updates).toEqual([''])
  })
})
