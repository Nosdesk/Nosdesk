// Live updates for the signed-in portal. The server sends only "request N
// changed" hints; each one invalidates that request's queries so they refetch
// through the portal API.
//
// One connection per browser, not per tab: tabs elect a leader with the Web
// Locks API, the leader holds the EventSource and relays hints to the others
// over a BroadcastChannel (browsers cap connections per origin). Without Web
// Locks every tab connects itself. Coming back to a tab refetches once, since a
// backgrounded stream may have gone quiet.
//
// The stream outlives a signed-out session until the server's next heartbeat,
// and the browser then reconnects without one and gets a 401. So signing out
// closes it first, in every tab, and so does reaching the sign-in page.
import { onBeforeUnmount, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useQueryCache } from '@pinia/colada'

const CHANNEL = 'nosdesk-portal-events'
const LOCK = 'nosdesk-portal-events-leader'

type Hint = { kind: 'ticket'; id: number } | { kind: 'resync' } | { kind: 'signed-out' }

// The mounted instance's way to close the stream, for signing out.
let closeStream: (() => void) | null = null

/** Close live updates before the session ends, here and in the other tabs. */
export function closePortalEvents(): void {
  closeStream?.()
}

export function usePortalEvents(): void {
  const queryCache = useQueryCache()
  const router = useRouter()
  let source: EventSource | null = null
  let leader = false
  let channel: BroadcastChannel | null = null
  let releaseLock: (() => void) | null = null

  function apply(hint: Hint): void {
    if (hint.kind === 'signed-out') {
      disconnect()
      return
    }
    if (hint.kind === 'resync') {
      void queryCache.invalidateQueries({ key: ['portal'] })
      return
    }
    void queryCache.invalidateQueries({ key: ['portal', 'ticket', hint.id] })
    void queryCache.invalidateQueries({ key: ['portal', 'tickets'] })
  }

  function relay(hint: Hint): void {
    apply(hint)
    channel?.postMessage(hint)
  }

  function connect(): void {
    source = new EventSource('/api/portal/events')
    source.addEventListener('ticket', (e) => {
      const id = Number(JSON.parse((e as MessageEvent<string>).data)?.id)
      if (Number.isInteger(id)) relay({ kind: 'ticket', id })
    })
    source.addEventListener('resync', () => relay({ kind: 'resync' }))
  }

  // Closing also cancels the browser's pending reconnect.
  function disconnect(): void {
    source?.close()
    source = null
  }

  // Only the signed-in request pages listen (the help centre, guest form and
  // sign-in live here too). A 401 closes the stream for good, so try again
  // after each navigation. Whoever reaches sign-in has no session to listen
  // with.
  function ensure(): void {
    if (router.currentRoute.value.name === 'login') {
      disconnect()
      return
    }
    const signedInPage = router.currentRoute.value.path.startsWith('/tickets')
    if (leader && signedInPage && (!source || source.readyState === EventSource.CLOSED)) connect()
  }
  const stopAfterEach = router.afterEach(() => ensure())

  function onVisible(): void {
    if (document.visibilityState === 'visible') apply({ kind: 'resync' })
  }

  onMounted(() => {
    closeStream = () => relay({ kind: 'signed-out' })
    document.addEventListener('visibilitychange', onVisible)
    if (typeof BroadcastChannel !== 'undefined') {
      channel = new BroadcastChannel(CHANNEL)
      channel.onmessage = (e: MessageEvent<Hint>) => apply(e.data)
    }
    if (navigator.locks) {
      void navigator.locks.request(
        LOCK,
        () =>
          new Promise<void>((resolve) => {
            releaseLock = resolve
            leader = true
            ensure()
          }),
      )
    } else {
      leader = true
      ensure()
    }
  })

  onBeforeUnmount(() => {
    closeStream = null
    document.removeEventListener('visibilitychange', onVisible)
    stopAfterEach()
    disconnect()
    channel?.close()
    releaseLock?.()
  })
}
