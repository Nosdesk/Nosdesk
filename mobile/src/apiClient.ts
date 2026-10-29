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
import {
  apiBaseUrl,
  passRequestGates,
  refuseResponse,
  hostHeadersFor,
  rememberHostHeaders,
  transport,
} from '@nosdesk/core/transport'
import { refreshSession } from '@nosdesk/core/services/session'
import { ACCESS_EXPIRES_IN_HEADER, noteAccessExpiry } from '@nosdesk/core/services/sessionSchedule'
import axios, { CanceledError } from 'axios'
import type { AxiosError, AxiosResponse, InternalAxiosRequestConfig } from 'axios'
import { tauriHttpAdapter } from './tauriHttpAdapter'
import { setSession } from './transport'

type RetryConfig = InternalAxiosRequestConfig & { _retry?: boolean }

/**
 * A sign-in (password, second factor, passkey, recovery code) answers in bearer
 * mode with the session's tokens in its body and no cookie. Hold them before
 * the caller continues, whichever sign-in screen sent the request.
 */
async function holdIssuedSession(response: AxiosResponse): Promise<AxiosResponse> {
  const body = response.data as { access_token?: unknown; refresh_token?: unknown } | null
  if (
    response.config.url?.startsWith('/auth/') &&
    typeof body?.access_token === 'string' &&
    typeof body.refresh_token === 'string'
  ) {
    await setSession(body.access_token, body.refresh_token)
  }
  return response
}

let registered = false

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

  apiClient.interceptors.request.use(async (config) => {
    // Held while a workspace switch is in progress.
    await passRequestGates()
    config.baseURL = apiBaseUrl()
    config.withCredentials = transport().auth.useCredentials
    // Use AxiosHeaders.set (not Object.assign) so values land in the instance's
    // canonical store that the native adapter serialises via toJSON.
    const authHeaders = transport().auth.authHeaders()
    for (const [key, value] of Object.entries(authHeaders)) {
      config.headers.set(key, value)
    }
    // Host per-request headers (workspace selection + diagnostics) from the
    // seam: the web apiConfig attaches these, but this bootstrap cleared it, so
    // apply the composed union here. A retry after a refresh reuses its first
    // attempt's headers, so it can't move to another workspace.
    const hostHeaders = hostHeadersFor(config)
    for (const [key, value] of Object.entries(hostHeaders)) {
      config.headers.set(key, value)
    }
    rememberHostHeaders(config, hostHeaders)
    return config
  })

  apiClient.interceptors.response.use(
    (response) => {
      // A response to a request sent under a previous workspace must not land.
      const refused = refuseResponse(response.config)
      if (refused) return Promise.reject(new CanceledError(refused))
      noteAccessExpiry(response.headers[ACCESS_EXPIRES_IN_HEADER])
      return holdIssuedSession(response)
    },
    async (error: AxiosError) => {
      if (axios.isCancel(error)) return Promise.reject(error)
      const refused = refuseResponse(error.config)
      if (refused) return Promise.reject(new CanceledError(refused))
      const original = error.config as RetryConfig | undefined
      if (error.response?.status !== 401 || !original || original._retry) {
        return Promise.reject(error)
      }
      original._retry = true
      // One in-flight refresh, shared with the sync runtime. A rejected one
      // has already cleared the session (refreshSession); surface the 401.
      if ((await refreshSession()) === 'renewed') return apiClient(original)
      return Promise.reject(error)
    },
  )
}
