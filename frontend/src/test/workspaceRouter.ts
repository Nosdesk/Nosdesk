/**
 * A memory router built the way the app builds its own: every authenticated
 * route takes the optional workspace segment, and the slug carrier and the
 * workspace guard are installed. Component specs mount against it in both
 * routing modes, since on Cloud every app path starts with the workspace slug
 * (`/acme/tickets`) and self-hosted paths are bare (`/tickets`).
 *
 * A spec using this mocks the routing mode and the active slug itself, at
 * the top of the spec file, so the mock is hoisted for that file:
 *
 *   let routing: 'host' | 'path' = 'host'
 *   let slug: string | null = null
 *   vi.mock('@nosdesk/core/services/instanceConfig', () => ({
 *     fetchInstanceConfig: async () => {},
 *     getWorkspaceRouting: () => routing,
 *   }))
 *   vi.mock('@/stores/workspaceReset', () => ({
 *     resetWorkspaceScopedState: async () => {},
 *     enterWorkspace: async () => {},
 *   }))
 *   vi.mock('@/services/activeWorkspace', () => ({
 *     beginWorkspaceSwitch: () => {},
 *     activeWorkspaceSlug: () => slug,
 *     setActiveWorkspaceSlug: (s: string | null) => { slug = s },
 *   }))
 *
 * The guard enters a workspace only while a session is held, so the router is
 * built signed in: it configures a transport that holds one, unless the spec
 * configured its own.
 */
import { createMemoryHistory, createRouter, type RouteRecordRaw, type Router } from 'vue-router'
import {
  installSlugCarrier,
  installWorkspaceGuard,
  withWorkspaceRouting,
} from '@/router/workspaceRouting'
import { configureTransport, transport } from '@nosdesk/core/transport'

/** The two routing modes, and the prefix a path carries in each. */
export const routingModes = [
  { mode: 'host' as const, prefix: '' },
  { mode: 'path' as const, prefix: '/acme' },
]

const page = { template: '<div />' }

/** A route record for a page that renders nothing. */
export function stubRoute(path: string, name: string): RouteRecordRaw {
  return { path, name, component: page, meta: { requiresAuth: true } }
}

/** Configure a signed-in transport, as the app's bootstrap would, unless the
 *  spec already configured one. */
export function ensureSignedInTransport(): void {
  try {
    transport()
  } catch {
    configureTransport({
      baseUrl: '/api',
      collabWsBaseUrl: '/api/collaboration/ws',
      auth: {
        authHeaders: () => ({}),
        useCredentials: true,
        refresh: async () => 'renewed',
        hasSession: () => true,
        onSessionLost: () => {},
        endSession: async () => {},
      },
    })
  }
}

export function makeWorkspaceRouter(routes: RouteRecordRaw[]): Router {
  ensureSignedInTransport()
  const router = createRouter({
    history: createMemoryHistory(),
    routes: withWorkspaceRouting(routes),
  })
  installSlugCarrier(router)
  installWorkspaceGuard(router)
  return router
}
