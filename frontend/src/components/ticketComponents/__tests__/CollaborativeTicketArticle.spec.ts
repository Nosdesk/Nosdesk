import { afterEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The restore endpoint only authorises a restore; the editor applies it to the
// live document. So the note has to hand a restored revision to its editor.
const editor = vi.hoisted(() => ({
  restoreRevision: vi.fn(async (_revision: number) => {}),
  replaceAllWithEmbeddedDocument: vi.fn((_doc: { uuid: string; title: string }) => {}),
  isHeldBy: vi.fn((_stateVector: Uint8Array) => true),
}))
const docs = vi.hoisted(() => ({ createPageFromTicket: vi.fn(async (): Promise<unknown> => null) }))
const toast = vi.hoisted(() => ({ warning: vi.fn() }))

vi.mock('@/components/CollaborativeEditor.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      setup(_, { expose }) {
        expose({
          restoreRevision: editor.restoreRevision,
          replaceAllWithEmbeddedDocument: editor.replaceAllWithEmbeddedDocument,
          isHeldBy: editor.isHeldBy,
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
vi.mock('@/components/common/ConfirmModal.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      props: { show: Boolean },
      emits: ['confirm', 'close'],
      setup(props, { emit }) {
        return () =>
          props.show ? h('button', { 'data-test': 'confirm', onClick: () => emit('confirm') }) : null
      },
    }),
  }
})
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => toast }))
vi.mock('@/composables/useCollabDocId', async () => {
  const { computed } = await import('vue')
  return { useCollabDocId: () => computed(() => 'ws-1_ticket-1') }
})
vi.mock('@/sync/stores/tickets', () => ({
  useSyncTicketsStore: () => ({ byId: () => ({ value: { uuid: 'ticket-1' } }) }),
}))
vi.mock('@nosdesk/core/services/documentationService', () => ({
  listDocsForTicket: async () => [],
  createPageFromTicket: docs.createPageFromTicket,
}))
vi.mock('@nosdesk/core/apiClient', () => ({ default: { get: vi.fn() } }))

import CollaborativeTicketArticle from '@/components/ticketComponents/CollaborativeTicketArticle.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

const page = { id: 5, uuid: 'page-5', title: 'Documentation: Ticket #7', slug: 'ticket-7' }

async function mountArticle() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  await router.push('/')
  wrapper = mountWithProviders(CollaborativeTicketArticle, { ticketId: 7, ticketNumber: 7 }, {}, [router])
  await flushPromises()
  return wrapper
}

async function promote(w: VueWrapper) {
  await w.get('button[aria-label="tickets-collaborative-article-convert-doc"]').trigger('click')
  await w.get('[data-test="confirm"]').trigger('click')
  await flushPromises()
}

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

  it('replaces the note with the page once the page holds all of it', async () => {
    docs.createPageFromTicket.mockResolvedValue({ page, created: true, stateVector: new Uint8Array([0]) })
    editor.isHeldBy.mockReturnValue(true)
    await promote(await mountArticle())

    expect(editor.replaceAllWithEmbeddedDocument).toHaveBeenCalledWith({ uuid: 'page-5', title: page.title })
    expect(toast.warning).not.toHaveBeenCalled()
  })

  it('keeps the note when the page is missing some of it', async () => {
    docs.createPageFromTicket.mockResolvedValue({ page, created: true, stateVector: new Uint8Array([0]) })
    editor.isHeldBy.mockReturnValue(false)
    await promote(await mountArticle())

    expect(editor.replaceAllWithEmbeddedDocument).not.toHaveBeenCalled()
    expect(toast.warning).toHaveBeenCalledWith('tickets-collaborative-article-promote-note-kept')
  })

  it('keeps the note when the ticket already had a page', async () => {
    docs.createPageFromTicket.mockResolvedValue({ page, created: false, stateVector: new Uint8Array([0]) })
    editor.isHeldBy.mockReturnValue(true)
    await promote(await mountArticle())

    expect(editor.replaceAllWithEmbeddedDocument).not.toHaveBeenCalled()
  })
})
