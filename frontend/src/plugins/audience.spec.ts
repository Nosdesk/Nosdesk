import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick } from 'vue'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import type { Plugin, PluginManifest } from '@nosdesk/core/types/plugin'
import '@/test/mountWithProviders'

// A manifest component's `audience` decides who the host shows it to. The
// loader's getters apply it, so every surface (slot, nav link, action, plugin
// page, modal) agrees. The viewer's tier comes from the real auth store.

const router = vi.hoisted(() => ({ push: vi.fn(), replace: vi.fn(), currentRoute: { value: {} } }))
vi.mock('@/router', () => ({ default: router, landAfterLogin: vi.fn() }))

const route = vi.hoisted(() => ({ params: {} as Record<string, string> }))
vi.mock('vue-router', async (importOriginal) => ({
  ...(await importOriginal<typeof import('vue-router')>()),
  useRoute: () => route,
}))

const enabled = vi.hoisted(() => ({ plugins: [] as unknown[] }))
vi.mock('@nosdesk/core/services/pluginService', () => ({
  default: { listEnabledPlugins: async () => enabled.plugins },
}))

const PLUGIN_UUID = 'p-1'

function testPlugin(): Plugin {
  const manifest = {
    manifest_version: 1,
    name: 'audience-test',
    displayName: 'Audience Test',
    version: '1.0.0',
    engines: { nosdesk: '>=1.0.0', plugin_api: '1' },
    permissions: [],
    settings: [],
    components: {
      railAll: { slot: 'asset.rail.panel', entry: 'railAll', context: ['asset'], label: 'For everyone' },
      railStaff: { slot: 'asset.rail.panel', entry: 'railStaff', context: ['asset'], audience: 'staff' },
      railAdmin: { slot: 'asset.rail.panel', entry: 'railAdmin', context: ['asset'], audience: 'admin' },
      pageAll: { slot: 'nav.item', entry: 'pageAll', audience: 'all' },
      pageAdmin: { slot: 'nav.item', entry: 'pageAdmin', label: 'Admin page', audience: 'admin' },
      headerStaff: {
        slot: 'ticket.header.action',
        entry: 'headerStaff',
        context: ['ticket'],
        audience: 'staff',
        action: { label: 'Open' },
      },
    },
  } as unknown as PluginManifest
  return { uuid: PLUGIN_UUID, name: 'audience-test', display_name: 'Audience Test', manifest } as unknown as Plugin
}

type Role = 'requester' | 'agent' | 'admin' | 'owner'

let pinia: Pinia

async function signInAs(role: Role) {
  const { useAuthStore } = await import('@/stores/auth')
  useAuthStore().user = { uuid: 'u-1', workspace_role: role, platform_role: null } as never
}

async function loadTestPlugin() {
  enabled.plugins = [testPlugin()]
  const loader = await import('./loader')
  await loader.loadPlugins()
  return loader
}

const names = (regs: { componentName: string }[]) => regs.map((r) => r.componentName).sort()

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
})

describe('meetsAudience', () => {
  it('maps each audience onto the viewer tier', async () => {
    const { meetsAudience } = await import('./audience')
    const requester = { isStaff: false, isAdmin: false }
    const agent = { isStaff: true, isAdmin: false }
    const admin = { isStaff: true, isAdmin: true }
    expect([undefined, 'all', 'staff', 'admin'].map((a) => meetsAudience(a as never, requester))).toEqual([
      true,
      true,
      false,
      false,
    ])
    expect(['all', 'staff', 'admin'].map((a) => meetsAudience(a as never, agent))).toEqual([true, true, false])
    expect(['all', 'staff', 'admin'].map((a) => meetsAudience(a as never, admin))).toEqual([true, true, true])
  })

  it('hides a value it does not know', async () => {
    const { meetsAudience } = await import('./audience')
    expect(meetsAudience('owner' as never, { isStaff: true, isAdmin: true })).toBe(false)
  })
})

describe('the loader getters', () => {
  it.each([
    ['requester', ['railAll'], ['pageAll'], []],
    ['agent', ['railAll', 'railStaff'], ['pageAll'], ['headerStaff']],
    ['admin', ['railAdmin', 'railAll', 'railStaff'], ['pageAdmin', 'pageAll'], ['headerStaff']],
    ['owner', ['railAdmin', 'railAll', 'railStaff'], ['pageAdmin', 'pageAll'], ['headerStaff']],
  ] as const)('show a %s only the components they may see', async (role, rail, pages, actions) => {
    await signInAs(role)
    const loader = await loadTestPlugin()
    expect(names(loader.getSlotRegistrations('asset.rail.panel'))).toEqual(rail)
    expect(names(loader.getSlotRegistrations('nav.item'))).toEqual(pages)
    expect(names(loader.getActionRegistrations('ticket.header.action'))).toEqual(actions)
    expect(loader.isComponentVisible(PLUGIN_UUID, 'pageAdmin', 'nav.item')).toBe(pages.includes('pageAdmin' as never))
  })

  it('treats a platform admin as admin', async () => {
    const { useAuthStore } = await import('@/stores/auth')
    useAuthStore().user = { uuid: 'u-1', workspace_role: null, platform_role: 'platform_admin' } as never
    const loader = await loadTestPlugin()
    expect(names(loader.getSlotRegistrations('nav.item'))).toEqual(['pageAdmin', 'pageAll'])
  })

  it('re-filters when the viewer role changes after the plugins load', async () => {
    await signInAs('requester')
    const loader = await loadTestPlugin()
    const { computed } = await import('vue')
    const rail = computed(() => names(loader.getSlotRegistrations('asset.rail.panel')))
    expect(rail.value).toEqual(['railAll'])
    await signInAs('admin')
    expect(rail.value).toEqual(['railAdmin', 'railAll', 'railStaff'])
  })
})

function mountWithI18n(component: ReturnType<typeof defineComponent>, props: Record<string, unknown> = {}) {
  const fluent = createFluentVue({ bundles: [new FluentBundle('en-US')] })
  return mount(component, {
    props,
    global: {
      plugins: [fluent, pinia],
      stubs: {
        NotFoundIllustration: true,
        PluginSlotItem: defineComponent({
          props: ['registration'],
          setup: (p) => () => h('div', { class: 'slot-item' }, p.registration.componentName),
        }),
        PluginSandboxFrame: defineComponent({
          props: ['component'],
          setup: (p) => () => h('div', { class: 'sandbox-frame' }, p.component.name),
        }),
      },
    },
  })
}

describe('the asset rail slot', () => {
  it('renders the rail contributions the viewer may see', async () => {
    await signInAs('agent')
    await loadTestPlugin()
    const PluginSlot = (await import('./components/PluginSlot.vue')).default
    const wrapper = mountWithI18n(PluginSlot, { target: 'asset.rail.panel', context: { asset: { id: 1 } } })
    expect(wrapper.findAll('.slot-item').map((n) => n.text())).toEqual(['railAll', 'railStaff'])

    await signInAs('requester')
    await nextTick()
    expect(wrapper.findAll('.slot-item').map((n) => n.text())).toEqual(['railAll'])
  })
})

describe('the plugin page', () => {
  it('shows the not-available state, not a frame, for a page above the viewer', async () => {
    await signInAs('agent')
    await loadTestPlugin()
    route.params = { uuid: PLUGIN_UUID, component: 'pageAdmin' }
    const PluginPageView = (await import('@/views/PluginPageView.vue')).default
    const wrapper = mountWithI18n(PluginPageView)
    expect(wrapper.find('.sandbox-frame').exists()).toBe(false)
    expect(wrapper.text()).toContain('plugin-page-unavailable')
  })

  it('renders the frame for a page the viewer may see', async () => {
    await signInAs('admin')
    await loadTestPlugin()
    route.params = { uuid: PLUGIN_UUID, component: 'pageAdmin' }
    const PluginPageView = (await import('@/views/PluginPageView.vue')).default
    const wrapper = mountWithI18n(PluginPageView)
    expect(wrapper.find('.sandbox-frame').text()).toBe('pageAdmin')
  })
})

describe('a plugin page opened before the plugins load', () => {
  it('renders the frame once they load', async () => {
    await signInAs('admin')
    enabled.plugins = []
    const loader = await import('./loader')
    await loader.loadPlugins()
    route.params = { uuid: PLUGIN_UUID, component: 'pageAdmin' }
    const PluginPageView = (await import('@/views/PluginPageView.vue')).default
    const wrapper = mountWithI18n(PluginPageView)
    expect(wrapper.find('.sandbox-frame').exists()).toBe(false)

    // Another plugin loads first, so the view re-renders between the two.
    const other = { ...testPlugin(), uuid: 'p-0', name: 'other' }
    enabled.plugins = [other, testPlugin()]
    await loader.loadPlugins()
    await nextTick()
    expect(wrapper.find('.sandbox-frame').text()).toBe('pageAdmin')
  })
})

describe('the plugin modal', () => {
  it('does not open a component above the viewer', async () => {
    await signInAs('requester')
    await loadTestPlugin()
    const { openPluginModal, closePluginModal } = await import('./usePluginModal')
    const PluginModalHost = (await import('./components/PluginModalHost.vue')).default
    const wrapper = mountWithI18n(PluginModalHost)
    openPluginModal({ pluginUuid: PLUGIN_UUID, componentName: 'headerStaff', slot: 'ticket.header.action' })
    await nextTick()
    expect(document.body.querySelector('.sandbox-frame')).toBeNull()

    await signInAs('agent')
    await nextTick()
    await nextTick()
    expect(document.body.querySelector('.sandbox-frame')?.textContent).toBe('headerStaff')
    closePluginModal()
    wrapper.unmount()
  })
})
