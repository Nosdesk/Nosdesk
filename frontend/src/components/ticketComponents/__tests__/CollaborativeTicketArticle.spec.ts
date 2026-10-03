import { afterEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The restore endpoint only authorises a restore; the editor applies it to the
// live document. So the note has to hand a restored revision to its editor.
const editor = vi.hoisted(() => ({ restoreRevision: vi.fn(async (_revision: number) => {}) }))

vi.mock('@/components/CollaborativeEditor.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      setup(_, { expose }) {
        expose({
          restoreRevision: editor.restoreRevision,
          exitRevisionView: () => {},
          isViewingRevision: false,
          currentRevisionNumber: null,
        })
        return () => h('div')
      },
    }),
  }
})
vi.mock('@/components/editor/RevisionList.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      emits: ['restored', 'selectRevision'],
      setup(_, { emit }) {
        return () => h('button', { 'data-test': 'restore', onClick: () => emit('restored', 3) })
      },
    }),
  }
})
vi.mock('@/composables/useCollabDocId', async () => {
  const { computed } = await import('vue')
  return { useCollabDocId: () => computed(() => 'ws-1_ticket-1') }
})
vi.mock('@/sync/stores/tickets', () => ({
  useSyncTicketsStore: () => ({ byId: () => ({ value: { uuid: 'ticket-1' } }) }),
}))
vi.mock('@nosdesk/core/services/documentationService', () => ({
  listDocsForTicket: async () => [],
  createPageFromTicket: async () => null,
}))
vi.mock('@nosdesk/core/apiClient', () => ({ default: { get: vi.fn() } }))

import CollaborativeTicketArticle from '@/components/ticketComponents/CollaborativeTicketArticle.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('CollaborativeTicketArticle', () => {
  it('hands a restored revision to the editor', async () => {
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
    })
    await router.push('/')
    wrapper = mountWithProviders(CollaborativeTicketArticle, { ticketId: 7, ticketNumber: 7 }, {}, [router])
    await flushPromises()

    await wrapper.get('button[aria-label="tickets-collaborative-article-revision-history"]').trigger('click')
    await wrapper.get('[data-test="restore"]').trigger('click')
    await flushPromises()

    expect(editor.restoreRevision).toHaveBeenCalledWith(3)
  })
})
