import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'

// The workspace guard owns the switch: a change of slug in the URL, by
// whatever route, resets the previous workspace's state and loads the
// next one's. The same slug is a no-op; the first slug of a session only
// enters.

const reset = vi.fn(async () => {})
const enter = vi.fn(async () => {})
vi.mock('@/stores/workspaceReset', () => ({
  resetWorkspaceScopedState: reset,
  enterWorkspace: enter,
}))
vi.mock('@nosdesk/core/services/instanceConfig', () => ({
  fetchInstanceConfig: async () => {},
  getWorkspaceRouting: () => 'path',
}))
let slug: string | null = null
const { beginSwitch } = vi.hoisted(() => ({ beginSwitch: vi.fn() }))
vi.mock('@/services/activeWorkspace', () => ({
  beginWorkspaceSwitch: beginSwitch,
  activeWorkspaceSlug: () => slug,
  setActiveWorkspaceSlug: (s: string | null) => {
    slug = s
  },
}))

import { installWorkspaceGuard, withWorkspaceRouting } from '@/router/workspaceRouting'

function makeRouter() {
  const page = { template: '<div />' }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: withWorkspaceRouting([
      { path: '/', name: 'home', component: page, meta: { requiresAuth: true } },
      { path: '/tickets', name: 'tickets', component: page, meta: { requiresAuth: true } },
    ]),
  })
  installWorkspaceGuard(router)
  return router
}

beforeEach(() => {
  reset.mockClear()
  enter.mockClear()
  beginSwitch.mockClear()
  slug = null
})

describe('workspace guard', () => {
  it('enters on the first slug, resets and enters on a different one, ignores the same', async () => {
    const router = makeRouter()
    await router.push('/mercury')
    expect(slug).toBe('mercury')
    expect(reset).not.toHaveBeenCalled()
    expect(enter).toHaveBeenCalledWith('mercury')

    await router.push('/mercury/tickets')
    expect(reset).not.toHaveBeenCalled()
    expect(beginSwitch).not.toHaveBeenCalled()
    expect(enter).toHaveBeenCalledTimes(1)

    // A typed URL, a back button, a deep link: no menu involved.
    await router.push('/venus/tickets')
    expect(reset).toHaveBeenCalledTimes(1)
    expect(slug).toBe('venus')
    expect(enter).toHaveBeenLastCalledWith('venus')
  })

  it('resets before the new slug is published, so the reset sees the old one', async () => {
    const router = makeRouter()
    await router.push('/mercury')
    let slugDuringReset: string | null = 'unset'
    let heldDuringReset = false
    reset.mockImplementationOnce(async () => {
      slugDuringReset = slug
      heldDuringReset = beginSwitch.mock.calls.length > 0
    })
    await router.push('/venus')
    expect(slugDuringReset).toBe('mercury')
    // Requests are held from before the reset until the new slug is set.
    expect(heldDuringReset).toBe(true)
    expect(slug).toBe('venus')
  })
})
