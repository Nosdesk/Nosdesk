import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, effectScope, nextTick, ref, type EffectScope } from 'vue'
import * as Y from 'yjs'
import { Awareness, applyAwarenessUpdate, encodeAwarenessUpdate } from 'y-protocols/awareness'

// The room the ticket's notes editor holds: this client's awareness, and the
// colleagues whose states reach it.
const room = vi.hoisted(() => ({ awareness: null as unknown as Awareness }))
vi.mock('@/stores/collabSession', () => ({
  useCollabSessionStore: () => ({
    sessionSnapshot: [],
    peek: () => ({ provider: { awareness: room.awareness } }),
  }),
}))
vi.mock('@/sync/stores/tickets', () => ({
  useSyncTicketsStore: () => ({ byId: () => computed(() => ({ uuid: 'ticket-uuid' })) }),
}))
vi.mock('@/composables/useCollabDocId', () => ({
  useCollabDocId: () => computed(() => 'ws-x_ticket-ticket-uuid'),
}))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ user: { uuid: 'me' } }) }))

import { useTicketPresence } from '../useTicketPresence'

/** A colleague's client in the room: its state reaches this client as the
 *  server relays it. */
function colleague(state: Record<string, unknown> | null) {
  const peer = new Awareness(new Y.Doc())
  peer.setLocalState(state)
  applyAwarenessUpdate(room.awareness, encodeAwarenessUpdate(peer, [peer.clientID]), 'server')
  return {
    set(next: Record<string, unknown> | null) {
      peer.setLocalState(next)
      applyAwarenessUpdate(room.awareness, encodeAwarenessUpdate(peer, [peer.clientID]), 'server')
    },
  }
}

let scope: EffectScope
function presence() {
  scope = effectScope()
  return scope.run(() => useTicketPresence(ref(1)))!
}

beforeEach(() => {
  vi.useFakeTimers()
  room.awareness = new Awareness(new Y.Doc())
  room.awareness.setLocalState({ user: { uuid: 'me', name: 'Me' } })
})
afterEach(() => {
  scope?.stop()
  vi.useRealTimers()
})

describe('who is on the ticket', () => {
  it('shows each colleague once, never yourself or a client without a user', async () => {
    const p = presence()
    colleague({ user: { uuid: 'sam' } })
    colleague({ user: { uuid: 'sam' } }) // Sam's second tab
    colleague({ user: { uuid: 'me' } }) // my other tab
    colleague({ cursor: null }) // no user: no one to show
    colleague({ user: { uuid: 'kim' } })
    await nextTick()
    expect(p.otherViewers.value.map((v) => v.user_uuid).sort()).toEqual(['kim', 'sam'])
  })

  it('drops a colleague who leaves', async () => {
    const p = presence()
    const sam = colleague({ user: { uuid: 'sam' } })
    await nextTick()
    expect(p.otherViewers.value).toHaveLength(1)
    sam.set(null)
    await nextTick()
    expect(p.otherViewers.value).toHaveLength(0)
  })
})

describe('what a colleague is typing', () => {
  it('shows over the saved title, and lingers until the saved one changes', async () => {
    const p = presence()
    const saved = ref('Printer')
    const shown = p.previewOver('title', () => saved.value)
    const sam = colleague({ user: { uuid: 'sam' } })
    expect(shown.value).toBeNull()

    sam.set({ user: { uuid: 'sam' }, preview: { field: 'title', value: 'Printer jam' } })
    await nextTick()
    expect(shown.value).toBe('Printer jam')

    // Sam saves: the preview stops before the saved title arrives.
    sam.set({ user: { uuid: 'sam' }, preview: null })
    await nextTick()
    expect(shown.value).toBe('Printer jam')
    saved.value = 'Printer jam'
    expect(shown.value).toBeNull()
  })

  it('stops lingering after a few seconds when nothing is saved', async () => {
    const p = presence()
    const shown = p.previewOver('title', () => 'Printer')
    const sam = colleague({ user: { uuid: 'sam' }, preview: { field: 'title', value: 'Print' } })
    await nextTick()
    sam.set({ user: { uuid: 'sam' } })
    await nextTick()
    expect(shown.value).toBe('Print')
    await vi.advanceTimersByTimeAsync(3000)
    expect(shown.value).toBeNull()
  })

  it('never shows your own typing back to you', async () => {
    const p = presence()
    const shown = p.previewOver('title', () => 'Printer')
    p.publishPreview('title', 'Mine')
    await vi.advanceTimersByTimeAsync(200)
    expect(room.awareness.getLocalState()?.preview).toEqual({ field: 'title', value: 'Mine' })
    expect(shown.value).toBeNull()
  })
})

describe('publishing what you type', () => {
  it('sends the latest value at most every 150 ms, and stopping at once', async () => {
    const p = presence()
    const sent: unknown[] = []
    room.awareness.on('change', () => sent.push(room.awareness.getLocalState()?.preview))
    p.publishPreview('title', 'P')
    p.publishPreview('title', 'Pr')
    p.publishPreview('title', 'Pri')
    expect(sent).toHaveLength(0)
    await vi.advanceTimersByTimeAsync(150)
    expect(sent).toEqual([{ field: 'title', value: 'Pri' }])

    p.publishPreview('title', null)
    expect(sent.at(-1)).toBeNull()
  })

  it('clears what you were typing when you leave the ticket', async () => {
    const p = presence()
    p.publishPreview('title', 'Half typed')
    await vi.advanceTimersByTimeAsync(150)
    scope.stop()
    expect(room.awareness.getLocalState()?.preview).toBeNull()
  })
})
