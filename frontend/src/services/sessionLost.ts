import { isLoggingOut } from '@/services/apiConfig'

/**
 * What the web app does when its session is lost (a refresh was rejected):
 * sign out locally and land on /login. Only pushing /login is not enough,
 * since the router sends a still-populated auth store straight back home, and
 * the workspace data would stay on screen. logout() clears both; its own
 * server call 401s quietly because it marks the session as tearing down.
 *
 * Registered with core's `setSessionLostHandler` at web bootstrap, so the
 * shared refresh runs it whoever hit the 401.
 */
export function redirectToLogin(): void {
  // A deliberate sign-out is already under way (a sync request can 401 while
  // it tears down); don't start a second one.
  if (isLoggingOut()) return
  // On the sign-in pages a lost session is expected; there is nowhere to send
  // the person.
  const path = window.location.pathname
  if (path.includes('/login') || path.includes('/onboarding')) return
  if (sessionStorage.getItem('redirecting-to-login')) return
  sessionStorage.setItem('redirecting-to-login', 'true')
  localStorage.removeItem('authProvider')

  setTimeout(async () => {
    try {
      const { useAuthStore } = await import('@/stores/auth')
      await useAuthStore().logout()
    } finally {
      sessionStorage.removeItem('redirecting-to-login')
    }
  }, 100)
}
