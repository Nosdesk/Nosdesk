import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { mountWithProviders } from '@/test/mountWithProviders'

// Pressing Create again while a new ticket is still being made joins that
// one: a phone that takes a moment to open the new ticket used to mint one per
// press.

const createEmptyTicket = vi.hoisted(() => vi.fn())
vi.mock('@nosdesk/core/services/ticketService', () => ({ default: { createEmptyTicket } }))
vi.mock('@nosdesk/core/sync/pool', () => ({ upsert: vi.fn() }))
vi.mock('@/sync/stores/tickets', () => ({ apiTicketToSync: (t: unknown) => t }))
const push = vi.hoisted(() => vi.fn(async () => {}))
vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))

import { useCreateTicketAction } from '@/composables/useCreateTicketAction'
import { usePageActionsStore } from '@nosdesk/core/stores/pageActions'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

function mountView() {
  const pinia = createPinia()
  setActivePinia(pinia)
  const View = defineComponent({
    setup() {
      useCreateTicketAction()
      return () => h('div')
    },
  })
  wrapper = mountWithProviders(View, {}, {}, [pinia])
  return usePageActionsStore()
}

function deferred<T>() {
  let resolve!: (v: T) => void
  let reject!: (e: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

describe('Create on a slow connection', () => {
  it('makes one ticket however many times it is pressed', async () => {
    const actions = mountView()
    const made = deferred<{ id: number; number: number }>()
    createEmptyTicket.mockReturnValue(made.promise)

    const presses = [1, 2, 3, 4].map(() => actions.invokeCreate())
    expect(actions.creating).toBe(true)
    made.resolve({ id: 41, number: 41 })
    await Promise.all(presses)
    await flushPromises()

    expect(createEmptyTicket).toHaveBeenCalledTimes(1)
    expect(push).toHaveBeenCalledTimes(1)
    expect(actions.creating).toBe(false)
  })

  it('stays busy until the new ticket is open, then takes the next press', async () => {
    const actions = mountView()
    createEmptyTicket.mockResolvedValue({ id: 42, number: 42 })
    const opened = deferred<void>()
    push.mockReturnValueOnce(opened.promise)

    const first = actions.invokeCreate()
    await flushPromises()
    expect(actions.creating).toBe(true)
    void actions.invokeCreate()
    expect(createEmptyTicket).toHaveBeenCalledTimes(1)

    opened.resolve()
    await first
    expect(actions.creating).toBe(false)
    await actions.invokeCreate()
    expect(createEmptyTicket).toHaveBeenCalledTimes(2)
  })

  it('takes a press again after a create fails', async () => {
    const actions = mountView()
    createEmptyTicket.mockRejectedValueOnce(new Error('offline'))
    vi.spyOn(console, 'error').mockImplementation(() => {})

    await actions.invokeCreate()
    expect(actions.creating).toBe(false)
    createEmptyTicket.mockResolvedValue({ id: 43, number: 43 })
    await actions.invokeCreate()
    expect(createEmptyTicket).toHaveBeenCalledTimes(2)
  })
})
