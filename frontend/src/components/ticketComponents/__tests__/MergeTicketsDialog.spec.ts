import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import { ConfigProvider, TooltipProvider } from 'reka-ui'
import { createMemoryHistory, createRouter } from 'vue-router'
import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'
// For its jsdom stubs (matchMedia, ResizeObserver) the dialog's controls need.
import '@/test/mountWithProviders'

const toast = { success: vi.fn(), warning: vi.fn(), error: vi.fn() }
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => toast }))
const mergeTickets = vi.fn().mockResolvedValue({})
vi.mock('@nosdesk/core/services/ticketService', () => ({
  mergeTickets: (...args: unknown[]) => mergeTickets(...args),
}))

import MergeTicketsDialog from '../MergeTicketsDialog.vue'

// Internal ids differ from the numbers people see: #98 is id 100, #99 is id 101.
const tickets = [
  { id: 100, number: 98, title: 'Printer jammed', workflow_state_id: 1 },
  { id: 101, number: 99, title: 'Printer still jammed', workflow_state_id: 1 },
]

const ModalStub = defineComponent({
  props: { show: Boolean, title: String },
  setup(props, { slots }) {
    return () => (props.show ? h('div', [h('h2', props.title), slots.default?.(), slots.footer?.()]) : null)
  },
})

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

function mountDialog(): VueWrapper {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:rest(.*)*', component: { render: () => null } }],
  })
  const Host = defineComponent({
    setup() {
      return () =>
        h(ConfigProvider, {}, () =>
          h(TooltipProvider, {}, () => h(MergeTicketsDialog, { open: true, selectedTickets: tickets })),
        )
    },
  })
  return mount(Host, {
    attachTo: document.body,
    global: {
      plugins: [createFluentVue({ bundles: [bundle] }), createPinia(), router],
      stubs: { Modal: ModalStub },
    },
  })
}

describe('MergeTicketsDialog', () => {
  it('shows ticket numbers, not internal ids', () => {
    wrapper = mountDialog()
    const sources = wrapper.find('ul').text()
    expect(sources).toContain('#99')
    expect(sources).not.toContain('#101')
    const note = (wrapper.find('textarea').element as HTMLTextAreaElement).value
    expect(note).toContain('Incoming from:')
    expect(note).toContain('- #99: Printer still jammed')
    expect(wrapper.text()).toContain('Merged 1 ticket into this one')
  })

  it('counts the merged-in tickets in its toast, by number', async () => {
    wrapper = mountDialog()
    const submit = wrapper.findAll('button').find((b) => b.text() === 'Merge 2 tickets')
    expect(submit).toBeTruthy()
    await submit!.trigger('click')
    await flushPromises()
    expect(mergeTickets).toHaveBeenCalledWith(
      expect.objectContaining({ destination_ticket_id: 100, source_ticket_ids: [101] }),
    )
    expect(toast.success).toHaveBeenCalledWith('Merged 1 ticket into #98')
  })
})
