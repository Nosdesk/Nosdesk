import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import * as pool from '@nosdesk/core/sync/pool'
import type { Page } from '@nosdesk/core/services/documentationService'

// Hovering or focusing a docs hub row warms the page's collaborative session,
// as ticket rows do, so the body is on its way before the click.

const WS_UUID = '0b6c3f0e-9a1d-4c2e-8f7a-5d4e3c2b1a09'
const PAGE_UUID = '6f1c2b8e-4d3a-4f5b-9c7d-2e1a0b9c8d7f'

const collab = vi.hoisted(() => ({ warm: vi.fn() }))
vi.mock('@/stores/collabSession', () => ({ useCollabSessionStore: () => collab }))
vi.mock('@/stores/myWorkspaces', () => ({
  useMyWorkspacesStore: () => ({ activeWorkspace: { workspace_uuid: WS_UUID } }),
}))

import DocumentationHubRow from '@/components/documentationComponents/DocumentationHubRow.vue'
import { mountWithProviders } from '@/test/mountWithProviders'

const DOC_ID = `ws-${WS_UUID}_doc-${PAGE_UUID}`

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  pool.reset()
  vi.clearAllMocks()
})

async function mountRow(props: Record<string, unknown>): Promise<VueWrapper> {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  await router.push('/documentation')
  wrapper = mountWithProviders(DocumentationHubRow, props, {}, [router])
  await flushPromises()
  return wrapper
}

describe('DocumentationHubRow', () => {
  it("warms a page row's session on hover and on focus", async () => {
    const page = { id: 5, uuid: PAGE_UUID, slug: 'payroll-runbook', title: 'Payroll runbook' } as Page
    const row = await mountRow({ page })

    await row.find('a').trigger('mouseenter')
    expect(collab.warm).toHaveBeenCalledWith(DOC_ID)
    await row.find('a').trigger('focus')
    expect(collab.warm).toHaveBeenCalledTimes(2)
  })

  it('warms a starred row by the page id the pool knows', async () => {
    pool.upsert('documentation_page', 5, { id: 5, uuid: PAGE_UUID, slug: 'payroll-runbook' })
    const row = await mountRow({ href: '/documentation/payroll-runbook', pageId: 5, title: 'Payroll runbook' })

    await row.find('a').trigger('mouseenter')
    expect(collab.warm).toHaveBeenCalledWith(DOC_ID)
  })

  it("warms nothing for a page it can't name", async () => {
    const row = await mountRow({ href: '/documentation/onboarding', pageId: 9, title: 'Onboarding' })

    await row.find('a').trigger('mouseenter')
    expect(collab.warm).not.toHaveBeenCalled()
  })
})
