import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'

// The project tabs read the open view from the route name, so the right tab
// is lit with and without the workspace slug in the path.

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

import ProjectTabBar from '@/components/views/ProjectTabBar.vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { makeWorkspaceRouter, routingModes, stubRoute } from '@/test/workspaceRouter'

const routes = [
  stubRoute('/projects/:id/gantt', 'project-gantt'),
  stubRoute('/projects/:id/cycles', 'project-cycles'),
  stubRoute('/projects/:id', 'project-detail'),
]

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const selected = () =>
  wrapper!
    .findAll('[role="tab"]')
    .filter((t) => t.attributes('aria-selected') === 'true')
    .map((t) => t.text())

async function clickTab(label: string) {
  const tab = wrapper!.findAll('[role="tab"]').find((t) => t.text() === label)
  if (!tab) throw new Error(`no tab ${label}`)
  await tab.trigger('mousedown', { button: 0, ctrlKey: false })
  await flushPromises()
}

describe.each(routingModes)('ProjectTabBar ($mode routing)', ({ mode, prefix }) => {
  beforeEach(() => {
    routing = mode
    slug = null
  })

  it.each([
    ['/projects/7', 'views-project-tab-board'],
    ['/projects/7/gantt', 'views-project-tab-gantt'],
    ['/projects/7/cycles', 'views-project-tab-cycles'],
  ])('lights the tab for %s', async (path, label) => {
    const router = makeWorkspaceRouter(routes)
    await router.push(`${prefix}${path}`)
    wrapper = mountWithProviders(ProjectTabBar, { projectId: 7 }, {}, [router])
    await flushPromises()
    expect(selected()).toEqual([label])
  })

  it('moves between the views inside the workspace', async () => {
    const router = makeWorkspaceRouter(routes)
    await router.push(`${prefix}/projects/7/gantt`)
    wrapper = mountWithProviders(ProjectTabBar, { projectId: 7 }, {}, [router])
    await flushPromises()

    await clickTab('views-project-tab-board')
    expect(router.currentRoute.value.path).toBe(`${prefix}/projects/7`)
    expect(selected()).toEqual(['views-project-tab-board'])
  })
})
