/**
 * Registers the transport interceptors on `@nosdesk/core`'s shared axios
 * instance, the mobile counterpart of the web's `apiConfig`.
 *
 * Scope is deliberately the transport concerns only: base URL, credential mode,
 * auth headers (from the seam), and the 401 -> refresh -> retry path. The web
 * `apiConfig` also registers correlation-id / diagnostics / SSE-client-id /
 * workspace-selection headers and richer error handling; those are app-level
 * concerns layered on later, not part of the bootstrap.
 */
import apiClient from '@nosdesk/core/apiClient'
import { apiBaseUrl, selectionHeaders, transport } from '@nosdesk/core/transport'
import type { AxiosError, InternalAxiosRequestConfig } from 'axios'
import { tauriHttpAdapter } from './tauriHttpAdapter'

type RetryConfig = InternalAxiosRequestConfig & { _retry?: boolean }

let registered = false
// Single in-flight refresh shared across concurrent 401s, so a burst of
// requests triggers exactly one token rotation.
let refreshing: Promise<boolean> | null = null

export function setupApiClient(): void {
  if (registered) return
  registered = true

  // Route the shared axios instance through Tauri's native HTTP client.
  apiClient.defaults.adapter = tauriHttpAdapter

  // The web apiConfig auto-registers its (cookie-oriented) interceptors when it
  // loads via stores/auth.ts, even in Tauri mode. Clear them so only the
  // bearer-mode interceptors below apply. Safe: this runs at bootstrap, before
  // any request fires.
  apiClient.interceptors.request.clear()
  apiClient.interceptors.response.clear()

  apiClient.interceptors.request.use((config) => {
    config.baseURL = apiBaseUrl()
    config.withCredentials = transport().auth.useCredentials
    // Use AxiosHeaders.set (not Object.assign) so values land in the instance's
    // canonical store that the native adapter serialises via toJSON.
    const authHeaders = transport().auth.authHeaders()
    for (const [key, value] of Object.entries(authHeaders)) {
      config.headers.set(key, value)
    }
    // Selection headers (Model-C workspace) from the seam: the web apiConfig
    // attaches these, but this bootstrap cleared it, so apply them here.
    const selection = selectionHeaders()
    for (const [key, value] of Object.entries(selection)) {
      config.headers.set(key, value)
    }
    return config
  })

  apiClient.interceptors.response.use(
    (response) => response,
    async (error: AxiosError) => {
      const original = error.config as RetryConfig | undefined
      if (error.response?.status !== 401 || !original || original._retry) {
        return Promise.reject(error)
      }
      original._retry = true
      refreshing ??= transport()
        .auth.refresh()
        .finally(() => {
          refreshing = null
        })
      const refreshed = await refreshing
      if (refreshed) return apiClient(original)
      // Session can't be renewed: clear local state and surface the 401.
      transport().auth.onSessionLost()
      return Promise.reject(error)
    },
  )
}
