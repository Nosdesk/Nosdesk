import type { SessionScheduleHost } from '@nosdesk/core/services/sessionSchedule'

const LOCK = 'nosdesk-session-refresh'
const STORAGE_KEY = 'nosdesk:access-expires-at'

/**
 * The browser side of the session refresh schedule, for the web app and the
 * native app's webview alike: a Web Lock so one tab refreshes at a time, the
 * expiry in `localStorage` so a tab that waited for the lock sees another
 * tab's renewal, and wake-ups when the page is shown or comes back online.
 * Without Web Locks or storage (an old browser, blocked site data), each tab
 * keeps its own schedule.
 */
export function browserSessionScheduleHost(): SessionScheduleHost {
  return {
    withLock(task) {
      const locks = typeof navigator !== 'undefined' ? navigator.locks : undefined
      return locks ? locks.request(LOCK, task) : task()
    },
    readShared() {
      try {
        const value = Number(localStorage.getItem(STORAGE_KEY))
        return Number.isFinite(value) && value > 0 ? value : null
      } catch {
        return null
      }
    },
    writeShared(at) {
      try {
        localStorage.setItem(STORAGE_KEY, String(at))
      } catch {
        // Private mode or blocked storage: each tab keeps its own schedule.
      }
    },
    listen({ wake, sharedChanged }) {
      const onVisibility = () => {
        if (document.visibilityState === 'visible') wake()
      }
      const onStorage = (event: StorageEvent) => {
        if (event.key === STORAGE_KEY) sharedChanged()
      }
      document.addEventListener('visibilitychange', onVisibility)
      window.addEventListener('online', wake)
      window.addEventListener('storage', onStorage)
      return () => {
        document.removeEventListener('visibilitychange', onVisibility)
        window.removeEventListener('online', wake)
        window.removeEventListener('storage', onStorage)
      }
    },
  }
}
