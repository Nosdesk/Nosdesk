// Axios client for the customer portal.
//
// Separate from the agent `apiClient`: the portal is its own session realm
// with its own cookies. Sends the portal cookies (withCredentials) and echoes
// the non-httpOnly `portal_csrf` cookie as the double-submit header, which the
// backend validates via `csrf_cookie_for_path` for `/api/portal/*`.
import axios from 'axios'

function portalCsrfToken(): string | null {
  const match = document.cookie.match(/portal_csrf=([^;]+)/)
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

// On an expired / missing portal session, bounce to the sign-in page (unless
// we're already there). Dynamic import avoids a router <-> api cycle.
portalApi.interceptors.response.use(
  (response) => response,
  async (error) => {
    if (error?.response?.status === 401) {
      const { default: router } = await import('./router')
      if (router.currentRoute.value.name !== 'login') {
        router.push('/login')
      }
    }
    return Promise.reject(error)
  },
)

export default portalApi
