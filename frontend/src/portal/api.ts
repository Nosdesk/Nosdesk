// Axios client for the customer portal.
//
// Separate from the agent `apiClient`: the portal is its own session realm
// with its own cookies. Sends the portal cookies (withCredentials) and echoes
// the non-httpOnly `portal_csrf` cookie as the double-submit header, which the
// backend validates via `csrf_cookie_for_path` for `/api/portal/*`.
import axios from 'axios'

function portalCsrfToken(): string | null {
  const match = document.cookie.match(/(?:^|;\s*)(?:__Host-)?portal_csrf=([^;]+)/)
  return match ? match[1] : null
}

const portalApi = axios.create({
  baseURL: '/api/portal',
  withCredentials: true,
  headers: { 'Content-Type': 'application/json' },
})

portalApi.interceptors.request.use((config) => {
  const token = portalCsrfToken()
  if (token) {
    config.headers['X-CSRF-Token'] = token
  }
  return config
})

// The access cookie lives 15 minutes; the refresh cookie a week. On a 401 (or
// a stale CSRF token, see below), rotate once (shared by every request that
// failed meanwhile) and retry; only when the refresh itself fails is the
// session gone, so bounce to sign-in.
let refreshing: Promise<boolean> | null = null

function refreshSession(): Promise<boolean> {
  refreshing ??= axios
    .post('/api/portal/auth/refresh', null, { withCredentials: true })
    .then(() => true)
    .catch(() => false)
    .finally(() => {
      refreshing = null
    })
  return refreshing
}

portalApi.interceptors.response.use(
  (response) => response,
  async (error) => {
    const original = error?.config
    const status = error?.response?.status
    // The CSRF cookie lives as long as the access cookie, so after a long idle
    // a write is refused for a missing token before auth is even checked.
    // Refreshing re-issues it, so treat that the same as an expired session.
    const code = error?.response?.data?.code
    const staleCsrf = status === 403 && (code === 'csrf_missing' || code === 'csrf_invalid')
    if ((status === 401 || staleCsrf) && original && !original._retried) {
      original._retried = true
      if (await refreshSession()) return portalApi(original)
      // Dynamic import avoids a router <-> api cycle.
      const { default: router } = await import('./router')
      if (router.currentRoute.value.name !== 'login') {
        router.push('/login')
      }
    }
    return Promise.reject(error)
  },
)

export default portalApi
