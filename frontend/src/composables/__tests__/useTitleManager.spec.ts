import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent, h, nextTick, reactive, ref } from 'vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { usePageTitle } from '@/composables/usePageTitle'

const route = reactive<{ name: string; meta: Record<string, unknown> }>({
  name: 'tickets',
  meta: { titleKey: 'route-title-tickets' },
})
vi.mock('vue-router', () => ({ useRoute: () => route }))
vi.mock('@nosdesk/core/services/ticketService', () => ({ previewTicketField: vi.fn(async () => {}) }))
vi.mock('@/stores/recentTickets', () => ({ useRecentTicketsStore: () => ({ updateTicketData: vi.fn() }) }))
vi.mock('@/utils/ticketNumbers', () => ({
  ticketNumber: (t: { id: number; number?: number }) => t.number ?? t.id,
}))

import { useTitleManager } from '@/composables/useTitleManager'

// One app-level reader, as App.vue holds it. Translations resolve to the
// message id under mountWithProviders.
let manager!: ReturnType<typeof useTitleManager>
mountWithProviders(
  defineComponent({
    setup() {
      manager = useTitleManager()
      return () => h('div')
    },
  }),
)

const claim = (setup: () => void) =>
  mount(
    defineComponent({
      setup() {
        setup()
        return () => h('div')
      },
    }),
  )

describe('useTitleManager', () => {
  it("names the tab after the route when no view claims it", async () => {
    await nextTick()
    expect(document.title).toBe('route-title-tickets | Nosdesk')
    expect(manager.pageTitle.value).toBe('route-title-tickets')
  })

  it("names the tab after the claiming view, and after the route once it's gone", async () => {
    const docs = claim(() => usePageTitle({ title: () => 'Documentation' }))
    await nextTick()
    expect(document.title).toBe('Documentation | Nosdesk')

    docs.unmount()
    route.name = 'ticket-view'
    route.meta = {}
    const ticket = claim(() => usePageTitle({ ticket: () => ({ id: 1, number: 12, title: 'Fix the printer' }) }))
    await nextTick()
    expect(document.title).toBe('#12 Fix the printer | Nosdesk')
    expect(manager.currentTicket.value?.title).toBe('Fix the printer')
    ticket.unmount()
    await nextTick()
    expect(document.title).toBe('Nosdesk')
    route.name = 'tickets'
    route.meta = { titleKey: 'route-title-tickets' }
  })

  it("saves a header rename through the claiming view, showing it until the view catches up", async () => {
    const name = ref('Roadmap')
    let release!: () => void
    const onTitleSave = vi.fn(
      (next: string) =>
        new Promise<void>((resolve) => {
          release = () => {
            name.value = next
            resolve()
          }
        }),
    )
    const project = claim(() => usePageTitle({ title: () => name.value, onTitleSave }))
    await nextTick()
    expect(manager.isCustomTitleEditable.value).toBe(true)

    const saving = manager.updateCustomTitle('Roadmap 2027')
    await nextTick()
    expect(onTitleSave).toHaveBeenCalledWith('Roadmap 2027')
    expect(document.title).toBe('Roadmap 2027 | Nosdesk')

    release()
    await saving
    await flushPromises()
    expect(document.title).toBe('Roadmap 2027 | Nosdesk')
    project.unmount()
  })

  it('offers an asset rename only while the asset view allows it', async () => {
    const editable = ref(false)
    const asset = claim(() =>
      usePageTitle({ device: () => ({ id: 4, name: 'MacBook Air' }), onTitleSave: async () => {}, titleEditable: editable }),
    )
    await nextTick()
    expect(document.title).toBe('#4 MacBook Air | Nosdesk')
    expect(manager.isDeviceTitleEditable.value).toBe(false)
    editable.value = true
    expect(manager.isDeviceTitleEditable.value).toBe(true)
    asset.unmount()
  })
})
