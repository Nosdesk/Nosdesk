import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { defineComponent, h, KeepAlive, nextTick, ref, toValue, type Component } from 'vue'
import { activeClaim, usePageTitle } from '@/composables/usePageTitle'

const titled = (title: () => string | null) =>
  defineComponent({
    setup() {
      usePageTitle({ title })
      return () => h('div')
    },
  })

const ticketView = (ticket: () => { id: number; title: string } | null) =>
  defineComponent({
    setup() {
      usePageTitle({ ticket })
      return () => h('div')
    },
  })

const shownTitle = () => toValue(activeClaim.value?.claim.title) ?? null
const shownTicket = () => toValue(activeClaim.value?.claim.ticket) ?? null

// Swaps the routed view the way <RouterView> does: one slot, one component.
function host(initial: Component) {
  const current = ref<Component>(initial)
  const wrapper = mount(defineComponent({ setup: () => () => h(current.value) }))
  return { wrapper, show: async (next: Component) => { current.value = next; await nextTick() } }
}

describe('usePageTitle', () => {
  it('hands the title to the ticket when Documentation is left for a ticket', async () => {
    const docs = titled(() => 'Documentation')
    const ticket = ticketView(() => ({ id: 12, title: 'Fix the printer' }))
    const { wrapper, show } = host(docs)
    expect(shownTitle()).toBe('Documentation')

    await show(ticket)
    expect(shownTitle()).toBeNull()
    expect(shownTicket()).toEqual({ id: 12, title: 'Fix the printer' })

    // And back again, which is what Nosbot saw stick.
    await show(docs)
    expect(shownTitle()).toBe('Documentation')
    expect(shownTicket()).toBeNull()
    wrapper.unmount()
    expect(activeClaim.value).toBeNull()
  })

  it('is right whichever of the two views settles first', () => {
    const scopeA = mount(titled(() => 'Documentation'))
    const scopeB = mount(ticketView(() => ({ id: 3, title: 'Toner' })))
    // Incoming view registered before the outgoing one unmounted.
    expect(shownTicket()).toEqual({ id: 3, title: 'Toner' })
    scopeA.unmount()
    expect(shownTicket()).toEqual({ id: 3, title: 'Toner' })
    scopeB.unmount()

    // Outgoing view unmounted before the incoming one registered.
    const scopeC = mount(titled(() => 'Documentation'))
    scopeC.unmount()
    expect(activeClaim.value).toBeNull()
    const scopeD = mount(ticketView(() => ({ id: 4, title: 'Paper jam' })))
    expect(shownTicket()).toEqual({ id: 4, title: 'Paper jam' })
    scopeD.unmount()
  })

  it('lets a view that is still loading fall through, then takes over', async () => {
    const loaded = ref<{ id: number; title: string } | null>(null)
    const outer = mount(titled(() => 'Projects'))
    const inner = mount(ticketView(() => loaded.value))
    expect(shownTitle()).toBe('Projects')

    loaded.value = { id: 9, title: 'Loaded' }
    await nextTick()
    expect(shownTicket()).toEqual({ id: 9, title: 'Loaded' })
    inner.unmount()
    outer.unmount()
  })

  it('steps aside while a KeepAlive holds its view, and returns with it', async () => {
    const list = defineComponent({
      name: 'CachedList',
      setup() {
        usePageTitle({ title: () => 'Tickets' })
        return () => h('div')
      },
    })
    const detail = ticketView(() => ({ id: 5, title: 'Detail' }))
    const current = ref<Component>(list)
    const wrapper = mount(
      defineComponent({ setup: () => () => h(KeepAlive, { include: ['CachedList'] }, [h(current.value)]) }),
    )
    expect(shownTitle()).toBe('Tickets')

    current.value = detail
    await nextTick()
    expect(shownTicket()).toEqual({ id: 5, title: 'Detail' })

    current.value = list
    await nextTick()
    expect(shownTitle()).toBe('Tickets')

    // A deactivated list with nothing above it gives the route its title.
    current.value = defineComponent({ render: () => h('div') })
    await nextTick()
    expect(activeClaim.value).toBeNull()
    wrapper.unmount()
  })
})
