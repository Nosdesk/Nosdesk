/**
 * A sync runtime opened for one workspace and asked for under another is torn
 * down and opened again. The live-updates stream goes with it: its token is
 * bound to the workspace it was asked for under, so a stream (or a pending
 * retry) carried into the next workspace is refused there.
 */
import { ref } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'

// No IndexedDB here: the runtime runs memory-only, which is enough to open it.
vi.mock('./idb', () => ({
  open: async () => {
    throw new Error('IndexedDB unavailable')
  },
}))
vi.mock('./queue', () => ({ setIdbHandle: () => {}, flush: async () => {} }))
vi.mock('@/utils/collabLocalCache', () => ({ purgeAllCollabDocs: async () => {} }))
vi.mock('@nosdesk/core/services/session', () => ({ sessionGone: () => false }))
vi.mock('@/services/activeWorkspace', () => ({
  workspaceReady: () => false,
  workspaceReadyRef: ref(false),
}))
vi.mock('@nosdesk/core/services/ticketService', () => ({ default: {} }))
vi.mock('@/sync/stores/tickets', () => ({ apiTicketToSync: (t: unknown) => t }))
vi.mock('@/services/authFetch', () => ({
  authFetch: async () => new Response(null, { status: 404 }),
}))
vi.mock('@/services/collabToken', () => ({
  resetCollabToken: () => {},
  keepCollabTokenWarm: () => {},
}))
const bridge = vi.hoisted(() => ({
  attachSseBridge: vi.fn(),
  detachSseBridge: vi.fn(),
}))
vi.mock('@/sync/sseBridge', () => bridge)

const { ensureSyncRuntime, tearDown } = await import('./lifecycle')

afterEach(async () => {
  await tearDown()
})

describe('a sync runtime asked for under another workspace', () => {
  it('closes the live-updates stream before opening the new one', async () => {
    await ensureSyncRuntime('u1', null)
    expect(bridge.attachSseBridge).toHaveBeenCalledTimes(1)
    expect(bridge.detachSseBridge).not.toHaveBeenCalled()

    await ensureSyncRuntime('u1', 'acme')

    expect(bridge.detachSseBridge).toHaveBeenCalledTimes(1)
    expect(bridge.attachSseBridge).toHaveBeenCalledTimes(2)
    expect(bridge.detachSseBridge.mock.invocationCallOrder[0]).toBeLessThan(
      bridge.attachSseBridge.mock.invocationCallOrder[1],
    )
  })
})
