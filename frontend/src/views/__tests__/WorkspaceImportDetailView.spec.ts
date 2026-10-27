import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import { mountWithProviders } from '@/test/mountWithProviders'
import { job } from '@/components/import/workspaceImport/__tests__/fixtures'

const api = vi.hoisted(() => ({ get: vi.fn(), reset: vi.fn(), retryFiles: vi.fn(), invite: vi.fn() }))
vi.mock('@nosdesk/core/services/workspaceImportService', async (importOriginal) => {
  const real = await importOriginal<typeof import('@nosdesk/core/services/workspaceImportService')>()
  return {
    ...real,
    workspaceImportService: Object.fromEntries(
      Object.keys(api).map((k) => [k, (...a: unknown[]) => api[k as keyof typeof api](...a)]),
    ),
  }
})
vi.mock('@/components/import/workspaceImport/useWorkspaceName', async () => {
  const { computed } = await import('vue')
  return { useWorkspaceName: () => computed(() => 'Acme Helpdesk') }
})

import WorkspaceImportDetailView from '@/views/WorkspaceImportDetailView.vue'

let wrapper: VueWrapper | null = null
let router: Router

async function mountAt(path: string) {
  const view = { template: '<div />' }
  router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/admin/data-import/workspace', name: 'admin-workspace-import', component: view },
      { path: '/admin/data-import/workspace/:id', name: 'admin-workspace-import-detail', component: view },
      { path: '/admin/data-import', name: 'admin-data-import', component: view },
      { path: '/:any(.*)*', component: view },
    ],
  })
  for (const name of ['admin-email-delivery', 'admin-channels', 'admin-auth-providers', 'admin-webhooks', 'admin-plugins', 'admin-rules', 'admin-api-tokens', 'home']) {
    router.addRoute({ path: `/x/${name}`, name, component: view })
  }
  await router.push(path)
  await router.isReady()
  wrapper = mountWithProviders(WorkspaceImportDetailView, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
}

const find = (sel: string) => wrapper!.find(`[data-test="${sel}"]`)

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

describe('WorkspaceImportDetailView', () => {
  it('shows a past import: its state, where it came from, and Done', async () => {
    api.get.mockResolvedValue(job('imported'))
    await mountAt('/admin/data-import/workspace/wimp-1')
    expect(api.get).toHaveBeenCalledWith('wimp-1')
    expect(find('workspace-detail-state').text()).toContain('import-history-state-imported')
    expect(find('workspace-detail-meta').text()).toContain('acme-it-2026-10-18.nosdesk')
    expect(find('workspace-detail-meta').text()).toContain('workspace-import-detail-source')
    expect(find('workspace-done-complete').exists()).toBe(true)
    expect(find('workspace-checklist').exists()).toBe(true)
    expect(find('workspace-files-skipped').text()).toContain('workspace-import-files-skipped')
    // The wizard's own buttons stay in the wizard.
    expect(find('workspace-open').exists()).toBe(false)
  })

  it('shows a reset import, with the way to import again', async () => {
    api.get.mockResolvedValue(job('reset'))
    await mountAt('/admin/data-import/workspace/wimp-1')
    expect(find('workspace-detail-state').text()).toContain('workspace-import-state-reset')
    expect(find('workspace-done-reset').exists()).toBe(true)
    expect(find('workspace-reset-button').exists()).toBe(false)
  })

  it('says the reset window closed once the import is pruned', async () => {
    api.get.mockResolvedValue(
      job('imported', { pruned_at: '2026-11-21T00:00:00Z', actions: { check: false, confirm: false, retry_files: false, reset: false, invite: false } }),
    )
    await mountAt('/admin/data-import/workspace/wimp-1')
    expect(find('workspace-reset-closed').exists()).toBe(true)
    expect(find('workspace-checklist').exists()).toBe(false)
  })

  it('opens an import still on its way in the wizard', async () => {
    api.get.mockResolvedValue(job('previewed'))
    await mountAt('/admin/data-import/workspace/wimp-1')
    expect(router.currentRoute.value.name).toBe('admin-workspace-import')
    expect(router.currentRoute.value.query.job).toBe('wimp-1')
  })
})
