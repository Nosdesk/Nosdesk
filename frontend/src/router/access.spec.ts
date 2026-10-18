import { describe, it, expect } from 'vitest'
import type { RouteMeta } from 'vue-router'
import { canOpenRoute, type RouteViewer } from './access'

// The meta /admin/workspaces carries (router/index.ts).
const workspacesConsole = [{ meta: { platformAdminRequired: true } as RouteMeta }]

const viewer = (overrides: Partial<RouteViewer>): RouteViewer => ({
  isAdmin: true,
  isAuditReviewer: false,
  isPlatformAdmin: false,
  ...overrides,
})

describe('platform-admin routes', () => {
  it('turn away a workspace owner or admin without the platform role', () => {
    expect(canOpenRoute(workspacesConsole, viewer({ workspaceRole: 'owner' }))).toBe(false)
    expect(canOpenRoute(workspacesConsole, viewer({ workspaceRole: 'admin' }))).toBe(false)
  })

  it('open for a platform admin', () => {
    expect(canOpenRoute(workspacesConsole, viewer({ isPlatformAdmin: true, workspaceRole: 'admin' }))).toBe(true)
  })
})
