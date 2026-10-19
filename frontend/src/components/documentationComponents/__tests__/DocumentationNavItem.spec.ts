import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import type { Page } from '@nosdesk/core/services/documentationService'

// A docs sidebar row is lit when its page is open, with and without the
// workspace slug in the path.

let routing: 'host' | 'path' = 'host'
let slug: string | null = null
vi.mock('@nosdesk/core/services/instanceConfig', () => ({
  fetchInstanceConfig: async () => {},
  getWorkspaceRouting: () => routing,
}))
vi.mock('@/stores/workspaceReset', () => ({
  resetWorkspaceScopedState: async () => {},
  enterWorkspace: async () => {},
}))
vi.mock('@/services/activeWorkspace', () => ({
  beginWorkspaceSwitch: () => {},
  activeWorkspaceSlug: () => slug,
  setActiveWorkspaceSlug: (s: string | null) => {
    slug = s
  },
}))

import DocumentationNavItem from '@/components/documentationComponents/DocumentationNavItem.vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { makeWorkspaceRouter, routingModes, stubRoute } from '@/test/workspaceRouter'

const routes = [stubRoute('/documentation', 'documentation'), stubRoute('/documentation/:path', 'documentation-page')]

const page = (id: number, slug: string) => ({ id, slug, title: slug, children: [] }) as unknown as Page

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const lit = () => wrapper!.find('li > div').classes().includes('bg-surface')

describe.each(routingModes)('DocumentationNavItem ($mode routing)', ({ mode, prefix }) => {
  beforeEach(() => {
    routing = mode
    slug = null
  })

  it('is lit on its own page and not on another', async () => {
    const router = makeWorkspaceRouter(routes)
    await router.push(`${prefix}/documentation/onboarding`)
    wrapper = mountWithProviders(DocumentationNavItem, { page: page(1, 'onboarding') }, {}, [router])
    await flushPromises()
    expect(lit()).toBe(true)

    await router.push(`${prefix}/documentation/offboarding`)
    await flushPromises()
    expect(lit()).toBe(false)
  })
})
