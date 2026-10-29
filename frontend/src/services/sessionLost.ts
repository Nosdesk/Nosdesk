import { isLoggingOut } from '@/services/apiConfig'

/**
 * What the app does when its session is lost (a refresh was rejected):
 * clear the tab's session state and land on /login. Only pushing /login is not
 * enough, since the router sends a still-populated auth store straight back
 * home, and the workspace data would stay on screen. This is not a sign-out:
 * the session is already over, and an /auth/logout sent now would carry the
 * shared cookies of any session another tab has since signed in with.
 *
 * Registered with core's `setSessionLostHandler` at platform bootstrap, on web
 * and in the mobile app, so the shared refresh runs it whoever hit the 401.
 */
const REDIRECTING = 'redirecting-to-login'

/** Drop a redirect flag left by a page that unloaded mid-redirect. sessionStorage
 *  outlives a reload, and a flag left set would skip every later redirect in
 *  this tab. Called at startup, when no redirect can be under way. */
export function clearStaleRedirect(): void {
  sessionStorage.removeItem(REDIRECTING)
}

export function redirectToLogin(): void {
  // A deliberate sign-out is already under way (a sync request can 401 while
  // it tears down); don't start a second one.
  if (isLoggingOut()) return
  // On the sign-in pages a lost session is expected; there is nowhere to send
  // the person.
  const path = window.location.pathname
  if (path.includes('/login') || path.includes('/onboarding')) return
  if (sessionStorage.getItem(REDIRECTING)) return
  sessionStorage.setItem(REDIRECTING, 'true')
  localStorage.removeItem('authProvider')

  setTimeout(async () => {
    try {
      const { useAuthStore } = await import('@/stores/auth')
      await useAuthStore().sessionLost()
    } finally {
      sessionStorage.removeItem(REDIRECTING)
    }
  }, 100)
}
