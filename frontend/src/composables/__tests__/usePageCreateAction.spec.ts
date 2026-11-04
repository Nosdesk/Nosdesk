import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, KeepAlive, nextTick, ref, shallowRef, type Component } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'

import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { usePageActionsStore } from '@nosdesk/core/stores/pageActions'

// The tickets list is KeepAlive-cached for swipe-back. Coming back to it from
// a ticket activates it before the ticket page unmounts, and the ticket page's
// release used to clear the list's action: "New ticket" then did nothing.

const listCreate = vi.fn()
const ticketCreate = vi.fn()

const TicketsListView = defineComponent({
  name: 'TicketsListView',
  setup() {
    usePageCreateAction(listCreate)
    return () => h('div', 'list')
  },
})
const TicketView = defineComponent({
  name: 'TicketView',
  setup() {
    usePageCreateAction(ticketCreate)
    return () => h('div', 'ticket')
  },
})

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

/** The app's router view: only the list is cached, as in App.vue. */
function mountApp() {
  setActivePinia(createPinia())
  const current = shallowRef<Component>(TicketsListView)
  const key = ref('list')
  wrapper = mount(
    defineComponent({
      setup: () => () =>
        h(KeepAlive, { include: ['TicketsListView'] }, [h(current.value, { key: key.value })]),
    }),
  )
  const go = async (view: Component, name: string) => {
    current.value = view
    key.value = name
    await nextTick()
    await nextTick()
  }
  return { store: usePageActionsStore(), go }
}

describe('usePageCreateAction', () => {
  it("keeps the cached list's action after coming back from a ticket", async () => {
    const { store, go } = mountApp()
    await nextTick()

    await go(TicketView, 'ticket')
    await store.invokeCreate()
    expect(ticketCreate).toHaveBeenCalledTimes(1)

    await go(TicketsListView, 'list')
    await store.invokeCreate()
    expect(listCreate).toHaveBeenCalledTimes(1)
    expect(store.hasCreateAction).toBe(true)
  })

  it('clears the action when its view goes and nothing replaces it', async () => {
    const { store } = mountApp()
    await nextTick()
    expect(store.hasCreateAction).toBe(true)
    wrapper?.unmount()
    wrapper = null
    await nextTick()
    expect(store.hasCreateAction).toBe(false)
  })
})
