import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed } from 'vue'
import { PiniaColada } from '@pinia/colada'
import { createMemoryHistory, createRouter } from 'vue-router'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { KnowledgeGap } from '@nosdesk/core/services/knowledgeGapsService'

const service = vi.hoisted(() => ({
  listKnowledgeGaps: vi.fn(),
  getKnowledgeGap: vi.fn(async () => null),
  dismissKnowledgeGaps: vi.fn(async (ids: number[]) => ids),
  reopenKnowledgeGaps: vi.fn(async (ids: number[]) => ids),
  detectClusters: vi.fn(),
  detectFailedSearches: vi.fn(),
  detectStaleDocs: vi.fn(),
  resolveKnowledgeGap: vi.fn(),
  writeDocForGap: vi.fn(),
}))
const toast = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn(), info: vi.fn() }))
vi.mock('@nosdesk/core/services/knowledgeGapsService', () => ({ default: service, ...service }))
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => toast }))
vi.mock('@nosdesk/core/sync/stores/documentation', () => ({
  useSyncDocsStore: () => ({
    allPages: [],
    pageById: () => computed(() => null),
    pagesByStatus: () => computed(() => []),
  }),
}))
vi.mock('@/composables/useTitleManager', () => ({
  useTitleManager: () => ({ setCustomTitle: () => {} }),
}))
vi.mock('@/composables/useTicketNumberLookup', () => ({ numberForTicketId: () => undefined }))
vi.mock('@/utils/ticketNumbers', () => ({
  ticketPath: () => '/tickets/1',
  ticketPathForId: () => '/tickets/1',
  ticketNumberForId: async () => undefined,
}))
vi.mock('@/components/common/PullToRefresh.vue', () => ({ default: { render: () => null } }))

import DocumentationGapsView from '@/views/DocumentationGapsView.vue'

function gap(id: number, title: string, kind: KnowledgeGap['primary_signal_type'], impact: number): KnowledgeGap {
  return {
    id,
    title,
    description: null,
    status: 'open',
    assignee_uuid: null,
    resolved_page_id: null,
    draft_page_id: null,
    subject_page_id: null,
    evidence_count: 1,
    last_evidence_at: '2026-10-11T00:00:00Z',
    impact_score: impact,
    created_at: '2026-10-11T00:00:00Z',
    updated_at: '2026-10-11T00:00:00Z',
    created_by: null,
    dismissed_at: null,
    dismissed_by: null,
    resolved_at: null,
    primary_signal_type: kind,
  }
}

const GAPS = [
  gap(1, 'Searched: "printer offline"', 'failed_search', 3),
  gap(2, 'Ticket #12: VPN drops', 'manual_flag', 1),
  gap(3, 'Searched: "badge printer"', 'failed_search', 2),
]

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

async function mountView() {
  // The server's list: what is still open, so a dismissal reaches it.
  const open = new Set(GAPS.map((g) => g.id))
  service.listKnowledgeGaps.mockImplementation(async () => GAPS.filter((g) => open.has(g.id)))
  service.dismissKnowledgeGaps.mockImplementation(async (ids: number[]) => {
    ids.forEach((id) => open.delete(id))
    return ids
  })
  service.reopenKnowledgeGaps.mockImplementation(async (ids: number[]) => {
    ids.forEach((id) => open.add(id))
    return ids
  })
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/documentation', component: { template: '<div />' } },
      { path: '/documentation/gaps', name: 'documentation-gaps', component: { template: '<div />' } },
      { path: '/documentation/gaps/:id', name: 'documentation-gap-detail', component: { template: '<div />' } },
    ],
  })
  await router.push('/documentation/gaps')
  wrapper = mountWithProviders(DocumentationGapsView, {}, {}, [router, [PiniaColada, {}] as never])
  await flushPromises()
  return { wrapper, router }
}

const rows = () => Array.from(document.body.querySelectorAll<HTMLElement>('[data-testid="gap-row"]'))
const rowTitles = () => rows().map((r) => r.querySelector('p')?.textContent?.trim())

describe('DocumentationGapsView', () => {
  it('labels each row by its demand and the kind of evidence, not a signal count', async () => {
    await mountView()
    const [search, ticket] = rows()
    expect(search.textContent).toContain('docs-gaps-impact-search-count')
    expect(search.textContent).toContain('docs-gaps-signal-failed-search')
    expect(ticket.textContent).toContain('docs-gaps-impact-ticket-count')
    expect(ticket.textContent).toContain('docs-gaps-signal-manual-flag')
    expect(document.body.textContent).not.toContain('signal-count')
  })

  it('keeps refresh in the title row, and the hint whole, through a refresh', async () => {
    await mountView()
    const heading = document.body.querySelector('h2')!
    const titleRow = heading.parentElement!
    const refresh = document.body.querySelector<HTMLButtonElement>('[data-testid="gaps-refresh"]')!
    expect(titleRow.contains(refresh)).toBe(true)
    expect(refresh.getAttribute('aria-label')).toBe('docs-gaps-refresh')
    expect(refresh.textContent?.trim()).toBe('')

    const status = document.body.querySelector<HTMLElement>('[data-testid="gaps-status"]')!
    expect(status.classList.contains('truncate')).toBe(false)
    expect(status.textContent?.trim()).toBe('docs-gaps-detect-schedule')

    let finish: (v: unknown) => void = () => {}
    service.detectFailedSearches.mockReturnValue(new Promise((r) => (finish = r)))
    service.detectClusters.mockResolvedValue({ clusters_detected: 0, gaps_created: 0, gaps_updated: 0 })
    service.detectStaleDocs.mockResolvedValue({ clusters_detected: 0, gaps_created: 0, gaps_updated: 0 })
    refresh.click()
    await flushPromises()
    expect(refresh.disabled).toBe(true)
    expect(refresh.getAttribute('aria-label')).toBe('docs-gaps-refreshing')
    expect(titleRow.contains(refresh)).toBe(true)

    finish({ clusters_detected: 3, gaps_created: 0, gaps_updated: 1, gaps_retired: 8 })
    await flushPromises()
    expect(refresh.disabled).toBe(false)
    expect(status.textContent?.trim()).toBe('docs-gaps-detect-updated, docs-gaps-detect-retired')
  })

  it('dismisses a row from its own button, with Undo', async () => {
    await mountView()
    const dismiss = rows()[0].querySelector<HTMLButtonElement>('button[aria-label="docs-gaps-aria-dismiss"]')!
    dismiss.click()
    await flushPromises()

    expect(service.dismissKnowledgeGaps).toHaveBeenCalledWith([1])
    expect(rowTitles()).not.toContain('Searched: "printer offline"')
    expect(toast.success).toHaveBeenCalledTimes(1)
    const [title, , undo] = toast.success.mock.calls[0]
    expect(title).toBe('docs-gaps-dismissed-toast')

    await undo.handler()
    await flushPromises()
    expect(service.reopenKnowledgeGaps).toHaveBeenCalledWith([1])
    expect(rowTitles()).toContain('Searched: "printer offline"')
  })

  it('dismisses the selected rows from the bulk bar', async () => {
    await mountView()
    expect(document.body.querySelector('[data-testid="gaps-dismiss-selected"]')).toBeNull()
    for (const row of [rows()[0], rows()[2]]) {
      row.querySelector<HTMLElement>('[role="checkbox"]')!.click()
      await flushPromises()
    }

    const bulk = document.body.querySelector<HTMLButtonElement>('[data-testid="gaps-dismiss-selected"]')!
    bulk.click()
    await flushPromises()

    expect(service.dismissKnowledgeGaps).toHaveBeenCalledTimes(1)
    expect([...service.dismissKnowledgeGaps.mock.calls[0][0]].sort()).toEqual([1, 3])
    expect(rowTitles()).toEqual(['Ticket #12: VPN drops'])
    expect(document.body.querySelector('[data-testid="gaps-dismiss-selected"]')).toBeNull()
  })

  it("hides the bulk bar on a phone while a gap's detail covers the list", async () => {
    const { wrapper: view } = await mountView()
    rows()[0].querySelector<HTMLElement>('[role="checkbox"]')!.click()
    await flushPromises()
    const bulk = () => document.body.querySelector<HTMLElement>('[data-testid="gaps-bulk"]')!
    expect(bulk().classList.contains('hidden')).toBe(false)

    await view.setProps({ id: '2' })
    await flushPromises()
    expect(bulk().classList.contains('hidden')).toBe(true)
    expect(bulk().classList.contains('lg:contents')).toBe(true)
  })

  it('puts the row back when the dismissal fails', async () => {
    await mountView()
    service.dismissKnowledgeGaps.mockRejectedValueOnce(new Error('offline'))
    rows()[1].querySelector<HTMLButtonElement>('button[aria-label="docs-gaps-aria-dismiss"]')!.click()
    await flushPromises()

    expect(toast.error).toHaveBeenCalledWith('docs-gaps-dismiss-failed', 'offline')
    expect(rowTitles()).toContain('Ticket #12: VPN drops')
  })
})
