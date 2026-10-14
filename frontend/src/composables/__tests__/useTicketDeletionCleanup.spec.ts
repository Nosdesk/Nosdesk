/**
 * A deleted ticket's local collab copy goes: the delete names the ticket by
 * id, and its row is already out of the pool when observers run.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'
import { createMemoryHistory, createRouter } from 'vue-router'
import * as pool from '@nosdesk/core/sync/pool'
import { buildCollabDocId } from '@nosdesk/core/utils/collabDocId'
import type { SyncAction } from '@nosdesk/core/sync/types'

const WS = vi.hoisted(() => '0b6c3f0e-9a1d-4c2e-8f7a-5d4e3c2b1a09')
const purgeData = vi.hoisted(() => vi.fn(async () => {}))
vi.mock('@/stores/collabSession', () => ({ useCollabSessionStore: () => ({ purgeData }) }))
vi.mock('@/stores/myWorkspaces', () => ({
  useMyWorkspacesStore: () => ({ activeWorkspace: { workspace_uuid: WS } }),
}))
vi.mock('extendable-media-recorder', () => ({ MediaRecorder: class {}, register: async () => {} }))
vi.mock('extendable-media-recorder-wav-encoder', () => ({ connect: async () => ({}) }))

const { applySseFrame } = await import('@/sync/lifecycle')
const { useTicketDeletionCleanup } = await import('@/composables/useTicketDeletionCleanup')

const TICKET = { id: 41, number: 12, uuid: '3c4d5e6f-7a8b-4c9d-8e0f-1a2b3c4d5e6f', title: 'Printer jam' }

const deletion: SyncAction = {
  sync_id: 1,
  aggregate: 'ticket',
  aggregate_id: String(TICKET.id),
  op: 'D',
  event_type: 'ticket.deleted',
  schema_version: 1,
  data: { id: TICKET.id, number: TICKET.number },
  groups: ['workspace:1'],
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  pool.reset()
  vi.clearAllMocks()
})

describe('useTicketDeletionCleanup', () => {
  it('clears the collab copy of a ticket whose row the delete already removed', async () => {
    pool.upsert('ticket', TICKET.id, { ...TICKET })
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
    })
    await router.push('/tickets')
    const Host = defineComponent({
      setup() {
        useTicketDeletionCleanup()
        return () => h('div')
      },
    })
    wrapper = mount(Host, { global: { plugins: [createPinia(), PiniaColada, router] } })
    await flushPromises()

    applySseFrame([deletion], 1, 1)
    await flushPromises()

    expect(pool.has('ticket', TICKET.id)).toBe(false)
    expect(purgeData).toHaveBeenCalledWith(buildCollabDocId(WS, 'ticket', TICKET.uuid))
  })
})
