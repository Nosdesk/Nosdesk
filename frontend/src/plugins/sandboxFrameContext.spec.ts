/**
 * The sandbox frame pushes context only when the context the guest sees
 * changes. A guest whose `mount` returns no `update` is re-mounted on every
 * push, so a push with nothing new wipes its DOM mid-await. Opening a host
 * dialog over the plugin modal re-rendered the modal's slot, which handed the
 * frame a new `{ name, slot }` object and pushed an identical context.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, defineComponent, h, nextTick, reactive, ref } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import type { Plugin } from '@nosdesk/core/types/plugin'

const sdk = vi.hoisted(() => ({
  postInit: vi.fn(),
  postContext: vi.fn(),
  postTheme: vi.fn(),
}))
vi.mock('@nosdesk/plugin-sdk', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/plugin-sdk')>()),
  createRemoteHostApi: () => ({ dispose: () => {} }),
  watchPluginHeight: () => () => {},
  postInit: sdk.postInit,
  postContext: sdk.postContext,
  postTheme: sdk.postTheme,
}))
vi.mock('@nosdesk/core/services/pluginService', () => ({
  default: { getBundleToken: async () => ({ runtime_url: 'about:blank' }) },
}))
vi.mock('./sandboxHostApi', () => ({
  createHostApiImpl: () => ({ hostApi: {}, inproc: { _dispose: () => {} } }),
}))
vi.mock('./pluginInstances', () => ({ registerPluginInstance: () => () => {} }))
vi.mock('./theme', () => ({ snapshotPluginTheme: () => ({}) }))
vi.mock('./layout', () => ({ usePluginLayout: () => computed(() => ({ breakpoint: 'lg' })) }))
vi.mock('@/stores/theme', () => ({
  useThemeStore: () => ({ isDarkMode: false, effectiveTheme: null, currentTheme: 'default' }),
}))

const { default: PluginSandboxFrame } = await import('./components/PluginSandboxFrame.vue')

const plugin = { uuid: 'p-1', name: 'dialog-tester' } as unknown as Plugin

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  sdk.postInit.mockClear()
  sdk.postContext.mockClear()
})

/** A parent that, like the plugin modal, passes `component` as an inline
 * literal, so each of its re-renders hands the frame a new object. */
async function mountFrame() {
  const ticket = reactive({ id: 1, title: 'Printer jam' })
  const state = reactive({ context: { ticket } as Record<string, unknown>, actionActivated: 0 })
  const renders = ref(0)
  const Parent = defineComponent({
    setup: () => () =>
      h('div', { 'data-renders': renders.value }, [
        h(PluginSandboxFrame, {
          plugin,
          component: { name: 'action', slot: 'ticket.header.action' },
          context: state.context,
          actionActivated: state.actionActivated,
        }),
      ]),
  })
  wrapper = mount(Parent, { attachTo: document.body })
  await flushPromises()
  await wrapper.find('iframe').trigger('load')
  expect(sdk.postInit).toHaveBeenCalled()
  sdk.postContext.mockClear()
  return { ticket, state, renders }
}

describe('sandbox frame context pushes', () => {
  it('does not push when the parent re-renders with an equal component', async () => {
    const { renders } = await mountFrame()
    renders.value++
    await nextTick()
    renders.value++
    await nextTick()
    expect(sdk.postContext).not.toHaveBeenCalled()
  })

  it('does not push when the context is replaced by an equal one', async () => {
    const { state } = await mountFrame()
    state.context = { ticket: { id: 1, title: 'Printer jam' } }
    await nextTick()
    expect(sdk.postContext).not.toHaveBeenCalled()
  })

  it('pushes when a context field or the action counter changes', async () => {
    const { ticket, state } = await mountFrame()
    ticket.title = 'Printer fixed'
    await nextTick()
    expect(sdk.postContext).toHaveBeenCalledTimes(1)
    expect(sdk.postContext.mock.calls[0][1]).toMatchObject({ ticket: { title: 'Printer fixed' } })

    state.actionActivated = 1
    await nextTick()
    expect(sdk.postContext).toHaveBeenCalledTimes(2)
    expect(sdk.postContext.mock.calls[1][1]).toMatchObject({ actionActivated: 1 })
  })
})
