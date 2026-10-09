import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

// A note renders from its local copy straight away, so its connection only
// shows when something is wrong: a slow first connect, a dropped connection,
// or no connection at all.

const fake = vi.hoisted(() => {
  type Listener = (...args: unknown[]) => void

  class FakeProvider {
    wsconnected = false
    wsconnecting = false
    shouldConnect = false
    params: Record<string, string> = {}
    awareness = { setLocalState: () => {} }
    private listeners = new Map<string, Set<Listener>>()
    constructor(_url: string, _room: string, _doc: unknown, opts: { connect?: boolean } = {}) {
      providers.push(this)
      if (opts.connect !== false) this.connect()
    }
    on(event: string, fn: Listener) {
      if (!this.listeners.has(event)) this.listeners.set(event, new Set())
      this.listeners.get(event)!.add(fn)
    }
    off(event: string, fn: Listener) {
      this.listeners.get(event)?.delete(fn)
    }
    emit(event: string, args: unknown[] = []) {
      for (const fn of [...(this.listeners.get(event) ?? [])]) fn(...args)
    }
    /** The token each dial went out with. */
    connectedWith: string[] = []
    /** Inside `connection-close`, while the closing socket is still attached. */
    private closing = false
    connect() {
      this.shouldConnect = true
      // As y-websocket: with the closing socket still attached, connect only
      // re-arms `shouldConnect`; nothing is dialled yet.
      if (this.closing) return
      this.connectedWith.push(this.params.token)
      this.wsconnecting = true
      this.emit('status', [{ status: 'connecting' }])
    }
    disconnect() {
      this.shouldConnect = false
      this.wsconnecting = false
      this.wsconnected = false
      this.emit('status', [{ status: 'disconnected' }])
    }
    destroyed = false
    destroy() {
      this.destroyed = true
    }
    /** The socket opened. */
    open() {
      this.wsconnecting = false
      this.wsconnected = true
      this.emit('status', [{ status: 'connected' }])
    }
    /** The socket dropped; y-websocket retries on its own. */
    drop() {
      this.wsconnected = false
      this.wsconnecting = false
      this.closing = true
      this.emit('connection-close', [null, this])
      this.closing = false
      this.emit('status', [{ status: 'disconnected' }])
    }
    /** The server closed the socket with `code`. As y-websocket 3.1 does: a
     *  44xx close turns reconnecting off and is then reported as `closed`. */
    serverClose(code: number) {
      const wasConnected = this.wsconnected
      this.wsconnected = false
      this.wsconnecting = false
      this.closing = true
      this.emit('connection-close', [{ code, reason: '' }, this])
      this.closing = false
      if (wasConnected) this.emit('status', [{ status: 'disconnected' }])
      if (code >= 4400 && code < 4500) {
        this.shouldConnect = false
        this.emit('closed', [{ code, reason: '' }, this])
      }
    }
  }

  const providers: FakeProvider[] = []
  return { FakeProvider, providers }
})
const providers = fake.providers
vi.mock('y-websocket', () => ({ WebsocketProvider: fake.FakeProvider }))
vi.mock('y-indexeddb', () => ({ IndexeddbPersistence: class {}, clearDocument: vi.fn() }))

const token = vi.hoisted(() => ({
  cached: null as string | null,
  next: null as Promise<string> | null,
  /** Answers for the next fetches, in order; then `next`, then 't'. */
  queue: [] as Array<() => Promise<string>>,
  fetches: 0,
  resets: 0,
}))
vi.mock('@/services/collabToken', () => ({
  peekCollabToken: () => token.cached,
  getCollabToken: () => {
    token.fetches++
    return token.queue.shift()?.() ?? token.next ?? Promise.resolve('t')
  },
  discardCollabToken: () => {
    token.resets++
    token.cached = null
  },
}))

// Whether the app still holds a session: core's `sessionGone()` is true once
// the shared refresh was rejected or no session is held.
const session = vi.hoisted(() => ({ present: true }))
vi.mock('@nosdesk/core/services/session', () => ({ sessionGone: () => !session.present }))

import { useCollabSessionStore } from '@/stores/collabSession'
import { logger } from '@nosdesk/core/utils/logger'

const OPTS = { baseWsUrl: 'ws://test/collab' }

function deferred<T>() {
  let resolve!: (v: T) => void
  const promise = new Promise<T>((r) => (resolve = r))
  return { promise, resolve }
}

let store: ReturnType<typeof useCollabSessionStore>
beforeEach(() => {
  vi.useFakeTimers()
  setActivePinia(createPinia())
  store = useCollabSessionStore()
  providers.length = 0
  token.cached = null
  token.next = null
  token.queue = []
  token.fetches = 0
  token.resets = 0
  session.present = true
  localStorage.setItem('nosdesk:disable-idb-collab', '1')
})
afterEach(() => {
  store.destroyAll()
  vi.useRealTimers()
  Object.defineProperty(navigator, 'onLine', { value: true, configurable: true })
})

describe('a note opening', () => {
  it('is connecting, not disconnected, while its token is on the way', () => {
    const pending = deferred<string>()
    token.next = pending.promise
    store.acquire('doc-a', OPTS)

    expect(store.connectionStatus['doc-a']).toBe('connecting')
  })

  it('shows nothing for a quick connect, and says so when it is slow', async () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)

    await vi.advanceTimersByTimeAsync(3500)
    expect(store.connectionBadge['doc-a'] ?? null).toBeNull()
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.connectionBadge['doc-a']).toBe('connecting')

    providers[0].open()
    expect(store.connectionBadge['doc-a'] ?? null).toBeNull()
  })

  it('times a slow connect from opening the note, not from hovering its row', async () => {
    token.cached = 't'
    // Hovering the row warms the note's connection, which stays connecting.
    store.warm('doc-a', OPTS)
    await vi.advanceTimersByTimeAsync(3000)

    store.acquire('doc-a', OPTS)
    expect(store.connectionBadge['doc-a'] ?? null).toBeNull()
    await vi.advanceTimersByTimeAsync(3500)
    expect(store.connectionBadge['doc-a'] ?? null).toBeNull()
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.connectionBadge['doc-a']).toBe('connecting')
  })
})

describe('a note that was connected', () => {
  it('says it is reconnecting once a drop lasts', async () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    providers[0].open()

    providers[0].drop()
    await vi.advanceTimersByTimeAsync(500)
    expect(store.connectionBadge['doc-a'] ?? null).toBeNull()
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.connectionBadge['doc-a']).toBe('reconnecting')
  })

  it('says it is disconnected straight away when the device is offline', () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    providers[0].open()

    Object.defineProperty(navigator, 'onLine', { value: false, configurable: true })
    providers[0].drop()
    window.dispatchEvent(new Event('offline'))
    expect(store.connectionBadge['doc-a']).toBe('disconnected')
  })

  it('says it is disconnected when the server closes it for good', () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    providers[0].open()

    providers[0].drop()
    providers[0].shouldConnect = false
    providers[0].emit('closed', [{ code: 4403, reason: 'forbidden' }, providers[0]])
    expect(store.connectionBadge['doc-a']).toBe('disconnected')
  })
})

describe('a note the server refuses', () => {
  /** Open `doc-a`, connected, and count its connect attempts from here. */
  function openNote() {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    providers[0].open()
    return vi.spyOn(providers[0], 'connect')
  }

  it('says the viewer has no access, and stops trying', async () => {
    const connects = openNote()

    providers[0].serverClose(4403)
    await vi.advanceTimersByTimeAsync(60_000)

    expect(connects).not.toHaveBeenCalled()
    expect(providers[0].shouldConnect).toBe(false)
    expect(store.connectionBadge['doc-a']).toBe('disconnected')
    expect(store.connectionRefusal['doc-a']).toBe('no-access')
  })

  it('says the note is gone', () => {
    openNote()
    providers[0].serverClose(4404)
    expect(store.connectionRefusal['doc-a']).toBe('gone')
  })

  it('tries once more with a fresh token before saying the viewer is signed out', async () => {
    openNote()

    // The server upgrades a refused connection before closing it, so the
    // socket opens each time.
    providers[0].open()
    providers[0].serverClose(4401)
    await vi.advanceTimersByTimeAsync(0)
    expect(token.resets).toBe(1)
    expect(providers[0].shouldConnect).toBe(true)
    expect(store.connectionRefusal['doc-a'] ?? null).toBeNull()

    // Meanwhile the session ended.
    session.present = false
    providers[0].open()
    providers[0].serverClose(4401)
    await vi.advanceTimersByTimeAsync(60_000)
    expect(token.resets).toBe(1)
    expect(store.connectionRefusal['doc-a']).toBe('signed-out')
  })

  it('keeps fetching fresh tokens, spaced out, when a fresh one is refused but the session holds', async () => {
    const warn = vi.spyOn(logger, 'warn')
    openNote()
    await vi.advanceTimersByTimeAsync(0)
    token.next = Promise.resolve('fresh-1')

    providers[0].open()
    providers[0].serverClose(4401)
    await vi.advanceTimersByTimeAsync(0)
    expect(providers[0].connectedWith.at(-1)).toBe('fresh-1')
    token.next = Promise.resolve('fresh-2')

    // The fresh token is refused too: not signed out, not "no access".
    providers[0].open()
    providers[0].serverClose(4401)
    await vi.advanceTimersByTimeAsync(1500)
    expect(store.connectionRefusal['doc-a'] ?? null).toBeNull()
    expect(store.connectionBadge['doc-a']).toBe('reconnecting')
    expect(providers[0].connectedWith.at(-1)).toBe('fresh-1')
    expect(warn).toHaveBeenCalledWith(
      expect.stringContaining('refused a fresh token'),
      expect.anything(),
    )

    // After the backoff, another fresh token.
    await vi.advanceTimersByTimeAsync(1000)
    expect(providers[0].connectedWith.at(-1)).toBe('fresh-2')
    expect(store.connectionRefusal['doc-a'] ?? null).toBeNull()
    warn.mockRestore()
  })

  it('reconnects with a new token when the first fetch for one fails, and never says signed out', async () => {
    openNote()
    await vi.advanceTimersByTimeAsync(0)
    // The token fetch fails once (the API blipped), then works.
    token.queue = [() => Promise.reject(new Error('network')), () => Promise.resolve('fresh')]

    providers[0].open()
    providers[0].serverClose(4401)
    // Play the server: a connect with the refused token is refused again.
    for (let i = 0; i < 60; i++) {
      await vi.advanceTimersByTimeAsync(1000)
      const p = providers[0]
      if (p.wsconnecting && p.params.token === 't') {
        p.open()
        p.serverClose(4401)
      }
      expect(store.connectionRefusal['doc-a'] ?? null).toBeNull()
    }

    expect(providers[0].connectedWith.slice(1)).not.toContain('t')
    expect(providers[0].connectedWith.at(-1)).toBe('fresh')
    expect(providers[0].shouldConnect).toBe(true)
  })

  it('stays reconnecting while a token cannot be fetched, and tries again at once when back online', async () => {
    openNote()
    await vi.advanceTimersByTimeAsync(0)
    token.next = Promise.reject(new Error('network'))
    token.next.catch(() => {})

    providers[0].open()
    providers[0].serverClose(4401)
    await vi.advanceTimersByTimeAsync(5000)
    expect(store.connectionBadge['doc-a']).toBe('reconnecting')
    expect(store.connectionRefusal['doc-a'] ?? null).toBeNull()
    expect(providers[0].connectedWith.slice(1)).toEqual([])

    // Retries space out: a long outage doesn't fetch every few seconds.
    const before = token.fetches
    await vi.advanceTimersByTimeAsync(5 * 60_000)
    expect(token.fetches - before).toBeLessThan(15)

    token.next = Promise.resolve('fresh')
    window.dispatchEvent(new Event('online'))
    await vi.advanceTimersByTimeAsync(0)
    expect(providers[0].connectedWith.at(-1)).toBe('fresh')
  })

  it('says signed out when the token fetch fails because the session is gone', async () => {
    openNote()
    await vi.advanceTimersByTimeAsync(0)
    // The token POST was refused and so was the refresh: the app no longer
    // holds a session.
    token.next = Promise.reject(new Error('401'))
    token.next.catch(() => {})
    session.present = false

    providers[0].open()
    providers[0].serverClose(4401)
    await vi.advanceTimersByTimeAsync(0)
    expect(store.connectionRefusal['doc-a']).toBe('signed-out')

    await vi.advanceTimersByTimeAsync(60_000)
    expect(providers[0].connectedWith.slice(1)).toEqual([])
    expect(store.connectionRefusal['doc-a']).toBe('signed-out')
  })

  it('tries a fresh token again once the server has served the note since', async () => {
    openNote()
    providers[0].open()
    providers[0].serverClose(4401)
    await vi.advanceTimersByTimeAsync(0)

    providers[0].open()
    providers[0].emit('sync', [true])
    providers[0].serverClose(4401)
    await vi.advanceTimersByTimeAsync(0)
    expect(token.resets).toBe(2)
    expect(store.connectionRefusal['doc-a'] ?? null).toBeNull()
  })

  it('keeps retrying a connection that only dropped', async () => {
    openNote()
    providers[0].serverClose(1006)
    expect(store.connectionRefusal['doc-a'] ?? null).toBeNull()
    expect(providers[0].shouldConnect).toBe(true)
  })
})

describe('a workspace switch', () => {
  it("closes every open note's connection", () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    store.acquire('doc-b', OPTS)
    store.release('doc-b')

    store.closeAll()

    expect(providers.map((p) => p.destroyed)).toEqual([true, true])
    expect(store.sessionSnapshot).toEqual([])
    expect(store.connectionStatus).toEqual({})
  })
})
