// Axios client for the customer portal.
//
// Separate from the agent `apiClient`: the portal is its own session realm
// with its own cookies. Sends the portal cookies (withCredentials) and echoes
// the non-httpOnly `portal_csrf` cookie as the double-submit header, which the
// backend validates via `csrf_cookie_for_path` for `/api/portal/*`.
import axios from 'axios'

import { embedBearer, embedHome, isEmbed, signInEmbedded } from './embed'
import { signInQuery } from './signInRedirect'

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
  // Embedded, the visitor's token rides a header (no cookies in a third-party
  // frame; a bearer needs no CSRF token).
  const bearer = isEmbed ? embedBearer() : null
  if (bearer) {
    config.headers['Authorization'] = `Bearer ${bearer}`
    return config
  }
  const token = portalCsrfToken()
  if (token) {
    config.headers['X-CSRF-Token'] = token
  }
  return config
})

// The access cookie lives 15 minutes; the refresh cookie a week. On a 401,
// rotate once (shared by every request that failed meanwhile) and retry; only
// when the refresh itself fails is the session gone, so bounce to sign-in.
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
    if (status === 401 && original && !original._retried) {
      original._retried = true
      if (isEmbed) {
        // The token lapsed: ask the host for a fresh one.
        if (await signInEmbedded()) return portalApi(original)
        const { default: router } = await import('./router')
        if (router.currentRoute.value.path !== embedHome) router.push(embedHome)
        return Promise.reject(error)
      }
      if (await refreshSession()) return portalApi(original)
      // Dynamic import avoids a router <-> api cycle. Sign-in returns the
      // requester to the page they were on.
      const { default: router } = await import('./router')
      const current = router.currentRoute.value
      if (current.name !== 'login') {
        router.push({ path: '/login', query: signInQuery(current.fullPath) })
      }
    }
    return Promise.reject(error)
  },
)

export default portalApi
