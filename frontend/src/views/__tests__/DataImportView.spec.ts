import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'

import { mountWithProviders } from '@/test/mountWithProviders'

const env = vi.hoisted(() => ({ hosted: false, owner: true }))
vi.mock('@nosdesk/core/services/instanceConfig', async (importOriginal) => {
  const real = await importOriginal<typeof import('@nosdesk/core/services/instanceConfig')>()
  return {
    ...real,
    isHostedDeploymentRef: {
      get value() {
        return env.hosted
      },
    },
  }
})
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    get isOwner() {
      return env.owner
    },
  }),
}))
// The hub's cards are covered by their own specs.
vi.mock('@/components/import/RecentImportsCard.vue', () => ({
  default: { props: ['workspaceImports'], template: '<div data-test="recent" :data-workspace="String(!!workspaceImports)" />' },
}))
vi.mock('@/components/ContactSupportModal.vue', () => ({
  default: { props: ['isOpen'], template: '<div v-if="isOpen" data-test="contact-support" />' },
}))
vi.mock('@/components/import/SavedMatchesCard.vue', () => ({ default: { template: '<div />' } }))

import DataImportView from '@/views/DataImportView.vue'

let wrapper: VueWrapper | null = null

async function mountHub() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:any(.*)*', component: { template: '<div />' } }],
  })
  await router.push('/admin/data-import')
  await router.isReady()
  wrapper = mountWithProviders(DataImportView, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
}

const card = () => wrapper!.find('[data-test="hub-nosdesk-export"]')

beforeEach(() => {
  env.hosted = false
  env.owner = true
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('DataImportView: the Nosdesk export card', () => {
  it('opens the import here for the owner on a self-hosted server', async () => {
    await mountHub()
    expect(card().element.tagName).toBe('A')
    expect(card().attributes('href')).toBe('/admin/data-import/workspace')
    expect(card().text()).toContain('admin-data-import-nosdesk-description')
    expect(card().text()).toContain('admin-data-import-status-available')
    expect(wrapper!.find('[data-test="recent"]').attributes('data-workspace')).toBe('true')
  })

  it('names the owner to an admin who isn\'t one, and opens nothing', async () => {
    env.owner = false
    await mountHub()
    expect(card().element.tagName).toBe('DIV')
    expect(card().text()).toContain('admin-data-import-status-owner-only')
    expect(wrapper!.find('[data-test="recent"]').attributes('data-workspace')).toBe('false')
  })

  it('on Cloud, says Nosdesk support runs moves in and opens Contact support', async () => {
    env.hosted = true
    await mountHub()
    expect(card().element.tagName).toBe('BUTTON')
    expect(card().attributes('href')).toBeUndefined()
    expect(card().text()).toContain('admin-data-import-nosdesk-description-cloud')
    expect(card().text()).toContain('admin-data-import-status-support')
    expect(wrapper!.find('[data-test="contact-support"]').exists()).toBe(false)
    await card().trigger('click')
    expect(wrapper!.find('[data-test="contact-support"]').exists()).toBe(true)
    expect(wrapper!.find('[data-test="recent"]').attributes('data-workspace')).toBe('false')
  })
})

describe('DataImportView: groups and tiles', () => {
  const group = (name: 'once' | 'sync') => wrapper!.find(`[data-test="hub-group-${name}"]`)
  const tiles = (name: 'once' | 'sync') =>
    group(name).findAll('[data-test^="hub-"]').map((t) => t.attributes('data-test'))
  const tile = (test: string) => wrapper!.find(`[data-test="${test}"]`)

  it('splits one-off imports from directory syncs on a self-hosted server', async () => {
    await mountHub()
    expect(group('once').text()).toContain('admin-data-import-group-once')
    expect(tiles('once')).toEqual(['hub-nosdesk-export', 'hub-documentation', 'hub-spreadsheet'])
    expect(group('sync').text()).toContain('admin-data-import-group-sync')
    expect(tiles('sync')).toEqual(['hub-microsoft', 'hub-directory'])
  })

  it('links every tile to its page', async () => {
    await mountHub()
    expect(tile('hub-documentation').attributes('href')).toBe('/admin/data-import/documentation')
    expect(tile('hub-spreadsheet').attributes('href')).toBe('/admin/data-import/csv')
    expect(tile('hub-microsoft').attributes('href')).toBe('/admin/data-import/microsoft-graph')
    expect(tile('hub-directory').attributes('href')).toBe('/admin/ldap')
    expect(tile('hub-directory').text()).toContain('admin-data-import-ad-description')
    expect(tile('hub-directory').text()).not.toContain('admin-data-import-ad-description-cloud')
  })

  it('offers nothing that does not open', async () => {
    await mountHub()
    expect(wrapper!.text()).not.toContain('admin-data-import-api-title')
    expect(wrapper!.text()).not.toContain('admin-data-import-status-coming-soon')
  })

  it('on Cloud, leaves out Microsoft and says LDAP needs a server on the internet', async () => {
    env.hosted = true
    await mountHub()
    expect(tiles('once')).toEqual(['hub-nosdesk-export', 'hub-documentation', 'hub-spreadsheet'])
    expect(tiles('sync')).toEqual(['hub-directory'])
    expect(wrapper!.text()).not.toContain('admin-data-import-microsoft-title')
    expect(tile('hub-directory').attributes('href')).toBe('/admin/ldap')
    expect(tile('hub-directory').text()).toContain('admin-data-import-ad-description-cloud')
  })
})
