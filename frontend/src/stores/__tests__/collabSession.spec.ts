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
    /** y-websocket's backoff counter and last-message clock. */
    wsUnsuccessfulReconnects = 0
    wsLastMessageReceived = 0
    /** The attached socket, as y-websocket holds it while connecting or open. */
    get ws(): object | null {
      return this.wsconnecting || this.wsconnected ? {} : null
    }
    awareness = { setLocalState: () => {} }
    private listeners = new Map<string, Set<Listener>>()
    /** Updates handed to the socket, as y-websocket's doc `update` handler
     *  does (it queues them while disconnected and sends on sync). */
    sent: Uint8Array[] = []
    private onDocUpdate = (update: Uint8Array, origin: unknown) => {
      if (origin !== this) this.sent.push(update)
    }
    constructor(
      _url: string,
      _room: string,
      private doc: YDocLike,
      opts: { connect?: boolean } = {},
    ) {
      providers.push(this)
      doc.on('update', this.onDocUpdate)
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
      // As y-websocket: closing the socket reports `connection-close` (and
      // would reconnect if `shouldConnect` were set again meanwhile).
      if (this.wsconnected || this.wsconnecting) {
        this.wsconnecting = false
        this.wsconnected = false
        this.closing = true
        this.emit('connection-close', [{ code: 1000, reason: '' }, this])
        this.closing = false
      }
      this.emit('status', [{ status: 'disconnected' }])
    }
    destroyed = false
    destroy() {
      this.destroyed = true
      this.doc.off('update', this.onDocUpdate)
    }
    /** The socket opened. */
    open() {
      this.wsconnecting = false
      this.wsconnected = true
      this.wsLastMessageReceived = Date.now()
      this.emit('status', [{ status: 'connected' }])
    }
    /** y-websocket's backoff timer firing: it dials if still asked to. */
    dialScheduled() {
      if (!this.shouldConnect || this.ws) return
      this.connectedWith.push(this.params.token)
      this.wsconnecting = true
      this.emit('status', [{ status: 'connecting' }])
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

  /** An IndexedDB connection, as lib0 opens it: closed on `versionchange`
   *  (another tab deleting the database). */
  class FakeDb extends EventTarget {
    closed = false
    constructor() {
      super()
      this.addEventListener('versionchange', () => this.close())
    }
    close() {
      this.closed = true
    }
  }

  /** y-indexeddb's persistence, down to the part that matters here: its doc
   *  `update` handler writes in a transaction, which throws once the
   *  connection is closed. */
  class FakeIdb {
    /** A y-indexeddb version whose private internals moved. */
    static internalsChanged = false
    stored: Uint8Array[] = []
    destroyed = false
    readonly connection = new FakeDb()
    _db = Promise.resolve(this.connection)
    _storeUpdate = (update: Uint8Array, origin: unknown) => {
      if (origin === this) return
      if (this.connection.closed) {
        throw new DOMException(
          "Failed to execute 'transaction' on 'IDBDatabase': The database connection is closing.",
          'InvalidStateError',
        )
      }
      this.stored.push(update)
    }
    constructor(
      readonly name: string,
      private doc: YDocLike,
    ) {
      idbs.push(this)
      doc.on('update', this._storeUpdate)
      if (FakeIdb.internalsChanged) delete (this as { _db?: unknown })._db
    }
    destroy() {
      this.doc.off('update', this._storeUpdate)
      this.destroyed = true
      return this._db.then((db) => db.close())
    }
    clearData() {
      return this.destroy().then(() => clearDocument(this.name))
    }
  }

  const clearDocument = vi.fn(async (_name: string) => {})

  /** Web Locks, with a hook for locks another tab holds. */
  class FakeLocks {
    /** Lock names another tab holds shared (it has the doc open). */
    heldElsewhere = new Set<string>()
    /** Locks this tab holds, by name, with their mode and count. */
    held = new Map<string, { mode: LockMode; count: number }>()
    async request(
      name: string,
      opts: LockOptions,
      cb: (lock: { name: string; mode: LockMode } | null) => unknown,
    ) {
      const mode = opts.mode ?? 'exclusive'
      const mine = this.held.get(name)
      const busy =
        (this.heldElsewhere.has(name) && mode === 'exclusive') ||
        (mine !== undefined && (mode === 'exclusive' || mine.mode === 'exclusive'))
      if (busy) {
        if (opts.ifAvailable) return cb(null)
        // Queued until the holder lets go, or withdrawn by its signal.
        return new Promise((_resolve, reject) => {
          opts.signal?.addEventListener('abort', () =>
            reject(new DOMException('withdrawn', 'AbortError')),
          )
        })
      }
      this.held.set(name, { mode, count: (mine?.count ?? 0) + 1 })
      try {
        return await cb({ name, mode })
      } finally {
        const entry = this.held.get(name)!
        if (--entry.count === 0) this.held.delete(name)
      }
    }
  }

  const providers: FakeProvider[] = []
  const idbs: FakeIdb[] = []
  return { FakeProvider, providers, FakeIdb, idbs, clearDocument, FakeLocks }
})
const providers = fake.providers
const idbs = fake.idbs
vi.mock('y-websocket', () => ({ WebsocketProvider: fake.FakeProvider }))
vi.mock('y-indexeddb', () => ({
  IndexeddbPersistence: fake.FakeIdb,
  clearDocument: fake.clearDocument,
}))

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

// Whether the app holds a session (the transport's `hasSession()`: on the web,
// a CSRF cookie). A rejected refresh drops it; signing in sets it again.
const session = vi.hoisted(() => ({ present: true, refreshAnswer: 'rejected' as string }))
vi.mock('@nosdesk/core/transport', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/transport')>()),
  transport: () => ({
    auth: {
      hasSession: () => session.present,
      refresh: async () => {
        // A renewed session comes back with a new CSRF cookie.
        if (session.refreshAnswer === 'renewed') session.present = true
        return session.refreshAnswer
      },
      onSessionLost: () => {
        session.present = false
      },
    },
  }),
}))

import { useCollabSessionStore } from '@/stores/collabSession'
import { logger } from '@nosdesk/core/utils/logger'
import { purgeAllCollabDocs } from '@/utils/collabLocalCache'

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
  idbs.length = 0
  fake.clearDocument.mockClear()
  token.cached = null
  token.next = null
  token.queue = []
  token.fetches = 0
  token.resets = 0
  session.present = true
  session.refreshAnswer = 'rejected'
  localStorage.setItem('nosdesk:disable-idb-collab', '1')
})
afterEach(() => {
  setTabHidden(false)
  store.destroyAll()
  vi.useRealTimers()
  // Back online, so no test inherits another's offline state.
  Object.defineProperty(navigator, 'onLine', { value: true, configurable: true })
  window.dispatchEvent(new Event('online'))
})

/** Hide or show the tab, as the browser reports it. */
function setTabHidden(hidden: boolean) {
  Object.defineProperty(document, 'hidden', { value: hidden, configurable: true })
  Object.defineProperty(document, 'visibilityState', {
    value: hidden ? 'hidden' : 'visible',
    configurable: true,
  })
  document.dispatchEvent(new Event('visibilitychange'))
}

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

describe('signing in again after the session was rejected', () => {
  it('is not signed out, so a note whose token fetch fails stays reconnecting', async () => {
    const { refreshSession, sessionGone } = await import('@nosdesk/core/services/session')
    // The session expired: the shared refresh is rejected.
    expect(await refreshSession()).toBe('rejected')
    expect(sessionGone()).toBe(true)

    // Signed in again by passkey: the user is set directly and the server
    // sets a new CSRF cookie. Nothing else is told.
    session.present = true
    expect(sessionGone()).toBe(false)

    token.cached = 't'
    store.acquire('doc-a', OPTS)
    await vi.advanceTimersByTimeAsync(0)
    providers[0].open()
    token.next = Promise.reject(new Error('network'))
    token.next.catch(() => {})
    providers[0].serverClose(4401)
    await vi.advanceTimersByTimeAsync(5000)

    expect(store.connectionRefusal['doc-a'] ?? null).toBeNull()
    expect(store.connectionBadge['doc-a']).toBe('reconnecting')
  })
})

describe('a note opened with no CSRF cookie while the session is still valid', () => {
  it('connects once the session is refreshed, and never says signed out', async () => {
    const { refreshSession } = await import('@nosdesk/core/services/session')
    // The CSRF cookie expired (a tab idle across the upgrade) but the refresh
    // cookie is still good: the server renews it.
    session.present = false
    session.refreshAnswer = 'renewed'
    // As apiClient does: the token POST gets a 401, refreshes, and retries.
    token.queue = [
      async () => {
        if ((await refreshSession()) !== 'renewed') throw new Error('401')
        return 'fresh'
      },
    ]

    store.acquire('doc-a', OPTS)
    for (let i = 0; i < 10; i++) {
      await vi.advanceTimersByTimeAsync(500)
      expect(store.connectionRefusal['doc-a'] ?? null).toBeNull()
    }
    expect(providers[0].connectedWith).toEqual(['fresh'])
  })
})

// A note's local copy (y-indexeddb) is a cache. Another tab can delete it
// (the LRU prune, a sign-out purge), and the browser then closes this tab's
// connection to it; from then on every write to it throws.
describe("a note's local copy", () => {
  const TOUCHED = 'nosdesk:collab-idb-touched'
  const lockName = (docId: string) => `nosdesk:collab-idb:${docId}`

  function withLocks() {
    const locks = new fake.FakeLocks()
    Object.defineProperty(navigator, 'locks', { value: locks, configurable: true })
    return locks
  }

  /** `count` stores in the touch map, oldest first. */
  function seedTouched(count: number, prefix = 'ws-old-') {
    const map: Record<string, number> = {}
    for (let i = 0; i < count; i++) map[`${prefix}${i}`] = i + 1
    localStorage.setItem(TOUCHED, JSON.stringify(map))
  }

  function touched(): Record<string, number> {
    return JSON.parse(localStorage.getItem(TOUCHED) ?? '{}') as Record<string, number>
  }

  beforeEach(() => {
    localStorage.removeItem('nosdesk:disable-idb-collab')
    token.cached = 't'
  })
  afterEach(() => {
    fake.FakeIdb.internalsChanged = false
    Object.defineProperty(navigator, 'locks', { value: undefined, configurable: true })
    delete (globalThis as { isTauri?: boolean }).isTauri
    localStorage.removeItem(TOUCHED)
  })

  it('never stops an edit reaching the server once its connection closes', async () => {
    const { ydoc } = store.acquire('ws-a_ticket-1', OPTS)
    await vi.advanceTimersByTimeAsync(0)
    // The browser dropped the connection without an event.
    idbs[0].connection.close()

    expect(() => ydoc.getText('t').insert(0, 'edit')).not.toThrow()
    expect(providers[0].sent).toHaveLength(1)
  })

  it('turns itself off when another tab deletes it, and edits still reach the server', async () => {
    const warn = vi.spyOn(logger, 'warn')
    const { ydoc } = store.acquire('ws-a_ticket-1', OPTS)
    await vi.advanceTimersByTimeAsync(0)
    ydoc.getText('t').insert(0, 'before ')
    expect(idbs[0].stored).toHaveLength(1)

    idbs[0].connection.dispatchEvent(new Event('versionchange'))

    expect(() => ydoc.getText('t').insert(7, 'after')).not.toThrow()
    expect(providers[0].sent).toHaveLength(2)
    expect(idbs[0].destroyed).toBe(true)
    expect(idbs[0].stored).toHaveLength(1)
    expect(warn).toHaveBeenCalledWith(
      expect.stringContaining('local copy'),
      expect.objectContaining({ docId: 'ws-a_ticket-1' }),
    )
  })

  it('is left off, and the note still opens, if y-indexeddb internals changed', async () => {
    const warn = vi.spyOn(logger, 'warn')
    const locks = withLocks()
    fake.FakeIdb.internalsChanged = true

    const { ydoc } = store.acquire('ws-a_ticket-1', OPTS)
    store.acquire('ws-a_ticket-2', OPTS)
    await vi.advanceTimersByTimeAsync(0)

    expect(() => ydoc.getText('t').insert(0, 'edit')).not.toThrow()
    expect(providers[0].sent).toHaveLength(1)
    expect(locks.held.size).toBe(0)
    expect(warn.mock.calls.filter(([msg]) => String(msg).includes('internals'))).toHaveLength(1)
  })

  it('is not pruned while another tab has the note open', async () => {
    const locks = withLocks()
    seedTouched(50)
    locks.heldElsewhere.add(lockName('ws-old-0'))

    store.acquire('ws-new', OPTS)
    await vi.advanceTimersByTimeAsync(0)

    expect(fake.clearDocument.mock.calls.map(([name]) => name)).toEqual(['ws-old-1'])
    expect(Object.keys(touched())).toHaveLength(50)
    expect(touched()).toHaveProperty('ws-old-0')
    expect(touched()).not.toHaveProperty('ws-old-1')
  })

  it('holds the note open for other tabs while this tab has it, and lets go on close', async () => {
    const locks = withLocks()
    store.acquire('ws-a_ticket-1', OPTS)
    await vi.advanceTimersByTimeAsync(0)
    expect(locks.held.get(lockName('ws-a_ticket-1'))?.mode).toBe('shared')

    await store.closeAll()

    expect(locks.held.has(lockName('ws-a_ticket-1'))).toBe(false)
  })

  it('never holds up closing notes while its lock is queued behind another tab', async () => {
    const locks = withLocks()
    // Another tab is deleting this note's store, so the lock is queued.
    locks.held.set(lockName('ws-a_ticket-1'), { mode: 'exclusive', count: 1 })
    store.acquire('ws-a_ticket-1', OPTS)
    await vi.advanceTimersByTimeAsync(0)

    let closed = false
    void store.closeAll().then(() => (closed = true))
    await vi.advanceTimersByTimeAsync(0)

    expect(closed).toBe(true)
  })

  it('is not pruned at all where the browser cannot say which notes other tabs have open', async () => {
    seedTouched(50)

    store.acquire('ws-new', OPTS)
    await vi.advanceTimersByTimeAsync(0)

    expect(fake.clearDocument).not.toHaveBeenCalled()
    expect(Object.keys(touched())).toHaveLength(51)
  })

  it('is pruned in the app shell without Web Locks, except notes open in it', async () => {
    // The app shell is one webview: its own sessions are every open note.
    ;(globalThis as { isTauri?: boolean }).isTauri = true
    store.acquire('ws-old-0', OPTS)
    seedTouched(50)

    store.acquire('ws-new', OPTS)
    await vi.advanceTimersByTimeAsync(0)

    expect(fake.clearDocument.mock.calls.map(([name]) => name)).toEqual(['ws-old-1'])
    expect(touched()).toHaveProperty('ws-old-0')
    expect(touched()).not.toHaveProperty('ws-old-1')
  })

  it('is cleared on sign-out even while other tabs have the note open', async () => {
    const locks = withLocks()
    seedTouched(3)
    for (let i = 0; i < 3; i++) locks.heldElsewhere.add(lockName(`ws-old-${i}`))

    await purgeAllCollabDocs({ includeOpen: true })

    expect(fake.clearDocument.mock.calls.map(([name]) => name).sort()).toEqual([
      'ws-old-0',
      'ws-old-1',
      'ws-old-2',
    ])
    expect(localStorage.getItem(TOUCHED)).toBeNull()
  })

  it('is kept on a workspace switch while another tab has the note open', async () => {
    const locks = withLocks()
    seedTouched(3)
    locks.heldElsewhere.add(lockName('ws-old-1'))

    await purgeAllCollabDocs({ includeOpen: false })

    expect(fake.clearDocument.mock.calls.map(([name]) => name).sort()).toEqual([
      'ws-old-0',
      'ws-old-2',
    ])
    expect(Object.keys(touched())).toEqual(['ws-old-1'])
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

describe('a note in a tab that is hidden', () => {
  it('lets its connection go after a while, and comes back with a fresh token, never the expired one', async () => {
    token.cached = 'expiring'
    store.acquire('doc-a', OPTS)
    providers[0].open()

    setTabHidden(true)
    await vi.advanceTimersByTimeAsync(30_000)
    expect(providers[0].wsconnected).toBe(false)
    expect(providers[0].shouldConnect).toBe(false)

    // The token outlived its short life while the tab was hidden.
    token.cached = null
    token.next = Promise.resolve('fresh')
    setTabHidden(false)
    await vi.advanceTimersByTimeAsync(0)

    expect(token.fetches).toBe(1)
    expect(providers[0].connectedWith).toEqual(['expiring', 'fresh'])
  })

  it('keeps its connection when the tab is shown again soon', async () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    providers[0].open()

    setTabHidden(true)
    await vi.advanceTimersByTimeAsync(10_000)
    setTabHidden(false)
    await vi.advanceTimersByTimeAsync(60_000)

    expect(providers[0].wsconnected).toBe(true)
    expect(providers[0].connectedWith).toEqual(['t'])
  })

  it('dials once when it is reopened before the tab is shown', async () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    providers[0].open()

    setTabHidden(true)
    await vi.advanceTimersByTimeAsync(30_000)
    store.release('doc-a')
    // Reopened (a prewarm, or a route change) while the tab is still hidden.
    store.acquire('doc-a', OPTS)
    await vi.advanceTimersByTimeAsync(0)
    providers[0].open()
    setTabHidden(false)
    await vi.advanceTimersByTimeAsync(0)

    expect(providers[0].connectedWith).toEqual(['t', 't'])
  })

  it('stays disconnected once shown if it was closed meanwhile, until it is opened again', async () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    providers[0].open()

    setTabHidden(true)
    await vi.advanceTimersByTimeAsync(30_000)
    store.release('doc-a')
    setTabHidden(false)
    await vi.advanceTimersByTimeAsync(0)
    expect(providers[0].connectedWith).toEqual(['t'])

    store.acquire('doc-a', OPTS)
    await vi.advanceTimersByTimeAsync(0)
    expect(providers[0].connectedWith).toEqual(['t', 't'])
  })
})

/** Take the device offline or back online, as the browser reports it. */
function setOnline(online: boolean) {
  Object.defineProperty(navigator, 'onLine', { value: online, configurable: true })
  window.dispatchEvent(new Event(online ? 'online' : 'offline'))
}

/** Open a note, then lose the network long enough that its token ran out and
 *  the token retries backed off to their 30 s cap. */
async function outage() {
  token.cached = 't'
  store.acquire('doc-a', OPTS)
  await vi.advanceTimersByTimeAsync(0)
  providers[0].open()
  setOnline(false)
  providers[0].drop()
  token.cached = null
  token.next = Promise.reject(new Error('offline'))
  token.next.catch(() => {})
  providers[0].drop()
  await vi.advanceTimersByTimeAsync(3 * 60_000)
}

describe('a note coming back online', () => {
  it('fetches a token and dials at once, despite a pending 30 s backoff', async () => {
    await outage()
    const fetches = token.fetches
    const dials = providers[0].connectedWith.length

    token.next = Promise.resolve('fresh')
    setOnline(true)
    await vi.advanceTimersByTimeAsync(0)

    expect(token.fetches).toBe(fetches + 1)
    expect(providers[0].connectedWith.length).toBe(dials + 1)
    expect(providers[0].connectedWith.at(-1)).toBe('fresh')
  })

  it("retries soon when the first try after coming back fails, not after the outage's backoff", async () => {
    await outage()
    // The first request after the network returns fails (it isn't quite back),
    // the next one works.
    token.queue = [() => Promise.reject(new Error('not yet')), () => Promise.resolve('fresh')]
    setOnline(true)
    await vi.advanceTimersByTimeAsync(0)
    expect(providers[0].connectedWith.at(-1)).not.toBe('fresh')

    await vi.advanceTimersByTimeAsync(2_000)
    expect(providers[0].connectedWith.at(-1)).toBe('fresh')
  })

  it('fetches afresh instead of waiting on a token fetch still on its way from before', async () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    await vi.advanceTimersByTimeAsync(0)
    providers[0].open()
    setOnline(false)
    token.cached = null
    // A fetch started as the network went; it never answers.
    token.next = new Promise<string>(() => {})
    providers[0].drop()
    await vi.advanceTimersByTimeAsync(10_000)
    const fetches = token.fetches

    token.next = Promise.resolve('fresh')
    setOnline(true)
    await vi.advanceTimersByTimeAsync(0)

    expect(token.fetches).toBe(fetches + 1)
    expect(providers[0].connectedWith.at(-1)).toBe('fresh')
  })

  it('redials a socket still stuck connecting', async () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    await vi.advanceTimersByTimeAsync(0)
    providers[0].open()
    setOnline(false)
    providers[0].drop()
    // y-websocket's next dial hangs: the socket never opens or fails.
    providers[0].dialScheduled()
    expect(providers[0].wsconnecting).toBe(true)
    await vi.advanceTimersByTimeAsync(10_000)
    const dials = providers[0].connectedWith.length

    setOnline(true)
    await vi.advanceTimersByTimeAsync(0)

    expect(providers[0].connectedWith.length).toBe(dials + 1)
    expect(providers[0].wsconnecting).toBe(true)
  })

  it('redials a socket that heard nothing while the device was offline', async () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    await vi.advanceTimersByTimeAsync(0)
    providers[0].open()
    setOnline(false)
    await vi.advanceTimersByTimeAsync(20_000)
    // Still reads as connected: nothing told the socket the network went.
    expect(providers[0].wsconnected).toBe(true)
    const dials = providers[0].connectedWith.length

    setOnline(true)
    await vi.advanceTimersByTimeAsync(0)

    expect(providers[0].connectedWith.length).toBe(dials + 1)
  })

  it('leaves alone a socket that stayed connected and the device stayed online', async () => {
    token.cached = 't'
    store.acquire('doc-a', OPTS)
    await vi.advanceTimersByTimeAsync(0)
    providers[0].open()
    const dials = providers[0].connectedWith.length

    window.dispatchEvent(new Event('online'))
    setTabHidden(true)
    setTabHidden(false)
    await vi.advanceTimersByTimeAsync(0)

    expect(providers[0].connectedWith.length).toBe(dials)
    expect(providers[0].wsconnected).toBe(true)
  })

  it('resets the backoff of y-websocket itself', async () => {
    await outage()
    providers[0].wsUnsuccessfulReconnects = 40
    token.next = Promise.resolve('fresh')
    setOnline(true)
    await vi.advanceTimersByTimeAsync(0)
    expect(providers[0].wsUnsuccessfulReconnects).toBe(0)
  })
})

describe('a note in a tab shown again', () => {
  it('fetches a token at once when it was waiting out a token backoff', async () => {
    await outage()
    setOnline(true)
    // Back online but the API is still unreachable: the doc waits to retry.
    await vi.advanceTimersByTimeAsync(0)
    setTabHidden(true)
    await vi.advanceTimersByTimeAsync(1_000)
    const fetches = token.fetches

    token.next = Promise.resolve('fresh')
    setTabHidden(false)
    await vi.advanceTimersByTimeAsync(0)

    expect(token.fetches).toBe(fetches + 1)
    expect(providers[0].connectedWith.at(-1)).toBe('fresh')
  })
})
