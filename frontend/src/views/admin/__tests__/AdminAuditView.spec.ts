import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// Workspace owners and admins read their workspace's app events and
// changes; the instance-wide sign-in events are for platform admins and
// audit reviewers. The page offers only the tiers the viewer can read, so
// no chip leads to a refusal.

const auth = vi.hoisted(() => ({ isPlatformAdmin: false, isAuditReviewer: false }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => auth }))

const route = vi.hoisted(() => ({ query: {} as Record<string, string> }))
vi.mock('vue-router', () => ({
  useRoute: () => route,
  useRouter: () => ({ replace: vi.fn() }),
}))

const list = vi.hoisted(() => vi.fn())
vi.mock('@nosdesk/core/services/auditService', () => ({
  auditService: { list, export: vi.fn() },
}))

import AdminAuditView from '@/views/admin/AdminAuditView.vue'

let wrapper: VueWrapper | null = null

beforeEach(() => {
  list.mockResolvedValue({ entries: [], next_cursor: null })
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  route.query = {}
  auth.isPlatformAdmin = false
  auth.isAuditReviewer = false
  vi.clearAllMocks()
})

async function mountView() {
  wrapper = mountWithProviders(AdminAuditView, {}, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  return wrapper
}

function chips(page: VueWrapper): string[] {
  return page.findAll('button[aria-pressed]').map((b) => b.text())
}

describe('audit tiers by viewer', () => {
  it('shows a workspace admin the app and change tiers only', async () => {
    const page = await mountView()

    expect(chips(page)).toEqual(['admin-audit-tier-all', 'admin-audit-tier-app', 'admin-audit-tier-change'])
    expect(page.text()).not.toContain('admin-audit-filter-severity')
  })

  it('shows a platform admin every tier', async () => {
    auth.isPlatformAdmin = true
    const page = await mountView()

    expect(chips(page)).toContain('admin-audit-tier-auth')
    expect(page.text()).toContain('admin-audit-filter-severity')
  })

  it('shows an audit reviewer every tier', async () => {
    auth.isAuditReviewer = true
    const page = await mountView()

    expect(chips(page)).toContain('admin-audit-tier-auth')
  })

  it('opens a shared sign-in events link on all readable tiers for a workspace admin', async () => {
    route.query = { tier: '2', severity: 'warning' }
    const page = await mountView()

    expect(list).toHaveBeenCalled()
    for (const [query] of list.mock.calls) {
      expect(query.tier).toBeUndefined()
      expect(query.severity).toBeUndefined()
    }
    expect(page.find('button[aria-pressed="true"]').text()).toBe('admin-audit-tier-all')
  })

  it('shows the empty state, not an error, when the workspace has no entries', async () => {
    const page = await mountView()

    expect(page.text()).toContain('admin-audit-empty-title')
    expect(page.text()).not.toContain('admin-audit-error-load')
  })
})
