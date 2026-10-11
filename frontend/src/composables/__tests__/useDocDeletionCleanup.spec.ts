/**
 * A documentation page or collection the reader loses (deleted for good, or
 * no longer theirs to open) arrives as a bare delete; its local collab copy
 * goes, open or not, and a view showing it closes.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import * as pool from '@nosdesk/core/sync/pool'
import { buildCollabDocId } from '@nosdesk/core/utils/collabDocId'
import type { SyncAction } from '@nosdesk/core/sync/types'

const WS = vi.hoisted(() => '0b6c3f0e-9a1d-4c2e-8f7a-5d4e3c2b1a09')
const purgeData = vi.hoisted(() => vi.fn(async () => {}))
vi.mock('@/stores/collabSession', () => ({ useCollabSessionStore: () => ({ purgeData }) }))
vi.mock('@/stores/myWorkspaces', () => ({
  useMyWorkspacesStore: () => ({ activeWorkspace: { workspace_uuid: WS } }),
}))

const { applySseFrame } = await import('@/sync/lifecycle')
const { useDocDeletionCleanup } = await import('@/composables/useDocDeletionCleanup')

const PAGE = { id: 5, uuid: '6f1c2b8e-4d3a-4f5b-9c7d-2e1a0b9c8d7f', slug: 'payroll-runbook', title: 'Payroll runbook', status: 'published', collection_id: 3 }
const OTHER = { id: 6, uuid: '1d2e3f4a-5b6c-4d7e-8f9a-0b1c2d3e4f5a', slug: 'leave-policy', title: 'Leave policy', status: 'published', collection_id: 3 }
const COLLECTION = { id: 3, uuid: '9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d', slug: 'payroll', name: 'Payroll' }

const PAGE_DOC = buildCollabDocId(WS, 'doc', PAGE.uuid)
const OTHER_DOC = buildCollabDocId(WS, 'doc', OTHER.uuid)
const COLLECTION_DOC = buildCollabDocId(WS, 'collection', COLLECTION.uuid)

/** A retraction as the server sends it: the record's id and nothing else. */
const retraction = (aggregate: 'documentation_page' | 'documentation_collection', id: number): SyncAction => ({
  sync_id: 1,
  aggregate,
  aggregate_id: String(id),
  op: 'D',
  event_type: `${aggregate}.deleted`,
  schema_version: 1,
  data: { id },
  groups: ['workspace:1'],
})

const pageUpdate = (data: Record<string, unknown>): SyncAction => ({
  ...retraction('documentation_page', PAGE.id),
  op: 'U',
  event_type: 'documentation_page.metadata_changed',
  data: { id: PAGE.id, ...data },
})

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  pool.reset()
  vi.clearAllMocks()
})

async function start(at: string): Promise<Router> {
  pool.upsert('documentation_page', PAGE.id, { ...PAGE })
  pool.upsert('documentation_page', OTHER.id, { ...OTHER })
  pool.upsert('documentation_collection', COLLECTION.id, { ...COLLECTION })
  const view = { template: '<div />' }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/documentation', component: view },
      { path: '/documentation/collections/:slug', name: 'collection-view', component: view },
      { path: '/documentation/:path', name: 'documentation-page', component: view },
      { path: '/error/:code', name: 'error', component: view },
    ],
  })
  await router.push(at)
  const Host = defineComponent({
    setup() {
      useDocDeletionCleanup()
      return () => h('div')
    },
  })
  wrapper = mount(Host, { global: { plugins: [createPinia(), router] } })
  await flushPromises()
  return router
}

describe('useDocDeletionCleanup', () => {
  it('clears the local copy of a page that is not open', async () => {
    const router = await start('/documentation/leave-policy')

    applySseFrame([retraction('documentation_page', PAGE.id)], 1, 1)
    await flushPromises()

    expect(purgeData).toHaveBeenCalledWith(PAGE_DOC)
    expect(purgeData).not.toHaveBeenCalledWith(OTHER_DOC)
    expect(router.currentRoute.value.name).toBe('documentation-page')
  })

  it('clears the open page and replaces it with the not-found page', async () => {
    const router = await start('/documentation/payroll-runbook')

    applySseFrame([retraction('documentation_page', PAGE.id)], 1, 1)
    await flushPromises()

    expect(purgeData).toHaveBeenCalledWith(PAGE_DOC)
    expect(router.currentRoute.value.name).toBe('error')
    expect(router.currentRoute.value.params.code).toBe('404')
  })

  it('leaves an updated or trashed page alone', async () => {
    const router = await start('/documentation/payroll-runbook')

    applySseFrame([pageUpdate({ title: 'Payroll' }), pageUpdate({ status: 'deleted' })], 1, 1)
    await flushPromises()

    expect(purgeData).not.toHaveBeenCalled()
    expect(router.currentRoute.value.name).toBe('documentation-page')
  })

  it('clears a lost collection and closes its view', async () => {
    const router = await start('/documentation/collections/payroll')

    applySseFrame([retraction('documentation_collection', COLLECTION.id)], 1, 1)
    await flushPromises()

    expect(purgeData).toHaveBeenCalledWith(COLLECTION_DOC)
    expect(router.currentRoute.value.name).toBe('error')
  })

  it('does nothing when the pool is wiped for a workspace switch or a resync', async () => {
    const router = await start('/documentation/payroll-runbook')

    pool.reset()
    await flushPromises()

    expect(purgeData).not.toHaveBeenCalled()
    expect(router.currentRoute.value.name).toBe('documentation-page')
  })
})
