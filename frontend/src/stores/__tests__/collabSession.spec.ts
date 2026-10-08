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
    connect() {
      this.shouldConnect = true
      this.wsconnecting = true
      this.emit('status', [{ status: 'connecting' }])
    }
    disconnect() {
      this.shouldConnect = false
      this.wsconnecting = false
      this.wsconnected = false
      this.emit('status', [{ status: 'disconnected' }])
    }
    destroy() {}
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
      this.emit('connection-close', [null, this])
      this.emit('status', [{ status: 'disconnected' }])
    }
  }

  const providers: FakeProvider[] = []
  return { FakeProvider, providers }
})
const providers = fake.providers
vi.mock('y-websocket', () => ({ WebsocketProvider: fake.FakeProvider }))
vi.mock('y-indexeddb', () => ({ IndexeddbPersistence: class {}, clearDocument: vi.fn() }))

const token = vi.hoisted(() => ({ cached: null as string | null, next: null as Promise<string> | null }))
vi.mock('@/services/collabToken', () => ({
  peekCollabToken: () => token.cached,
  getCollabToken: () => token.next ?? Promise.resolve('t'),
}))

import { useCollabSessionStore } from '@/stores/collabSession'

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

    await vi.advanceTimersByTimeAsync(500)
    expect(store.connectionBadge['doc-a'] ?? null).toBeNull()
    await vi.advanceTimersByTimeAsync(2000)
    expect(store.connectionBadge['doc-a']).toBe('connecting')

    providers[0].open()
    expect(store.connectionBadge['doc-a'] ?? null).toBeNull()
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
