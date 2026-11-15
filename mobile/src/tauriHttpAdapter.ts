/**
 * Axios adapter backed by Tauri's native HTTP plugin.
 *
 * From a `tauri://` origin, requests to the API are cross-origin; the official
 * `@tauri-apps/plugin-http` issues them through the native HTTP client instead
 * of the webview, sidestepping mobile WebView CORS quirks (and scoped by the
 * `http` capability, see src-tauri/capabilities). The mobile bootstrap installs
 * this as `apiClient.defaults.adapter` so the shared frontend/core stay
 * Tauri-agnostic; nothing imports the plugin outside Tauri mode.
 *
 * Covers the JSON REST surface the app uses (method, headers, query params,
 * string bodies, json/text/blob/arraybuffer responses). It preserves the
 * 401-with-response shape the transport's refresh interceptor relies on. File
 * bodies go through the webview's XHR instead (below).
 */
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'
import axios, { AxiosError, AxiosHeaders } from 'axios'
import type { AxiosAdapter, AxiosResponse, InternalAxiosRequestConfig } from 'axios'

function buildUrl(config: InternalAxiosRequestConfig): string {
  const path = config.url ?? ''
  const base = config.baseURL ?? ''
  let url = /^https?:\/\//i.test(path)
    ? path
    : `${base.replace(/\/$/, '')}/${path.replace(/^\//, '')}`
  if (config.params) {
    // Drop null/undefined params (don't serialise them as the string
    // "undefined"); axios's default serializer omits them, so match it.
    const entries = Object.entries(config.params as Record<string, unknown>)
      .filter(([, v]) => v != null)
      .map(([k, v]) => [k, String(v)] as [string, string])
    const qs = new URLSearchParams(entries).toString()
    if (qs) url += (url.includes('?') ? '&' : '?') + qs
  }
  return url
}

function toHeaderRecord(config: InternalAxiosRequestConfig): Record<string, string> {
  const out: Record<string, string> = {}
  const json = config.headers?.toJSON?.() ?? {}
  for (const [key, value] of Object.entries(json as Record<string, unknown>)) {
    if (value != null) out[key] = String(value)
  }
  return out
}

/**
 * File bodies go through the webview's own XHR instead: WebKit streams a
 * file-backed `File` from disk, reports upload progress and aborts through the
 * request's signal, so a video never crosses the IPC bridge. The interceptors
 * on `apiClient` have already added the bearer and workspace headers, and the
 * API allows the app's origin and those headers cross-origin.
 */
const xhrAdapter = axios.getAdapter('xhr')

function isFileBody(body: unknown): boolean {
  return body instanceof FormData || body instanceof Blob
}

/**
 * The request headers the API allows cross-origin (`startup.rs`, the CORS
 * `allowed_headers`). The webview preflights the XHR, and one header outside
 * this list fails the whole upload, so diagnostics headers the native client
 * could send (correlation, trace and SSE client ids) are left off. Matches
 * servers back to 1.1.5.
 */
const CROSS_ORIGIN_HEADERS = new Set([
  'accept',
  'authorization',
  'content-type',
  'x-auth-mode',
  'x-csrf-token',
  'x-nosdesk-plugin',
  'x-nosdesk-workspace',
  'x-requested-with',
])

function dropDisallowedHeaders(config: InternalAxiosRequestConfig): void {
  for (const key of Object.keys(config.headers.toJSON())) {
    if (!CROSS_ORIGIN_HEADERS.has(key.toLowerCase())) config.headers.delete(key)
  }
}

export const tauriHttpAdapter: AxiosAdapter = async (config) => {
  const method = (config.method ?? 'get').toUpperCase()
  if (isFileBody(config.data) && method !== 'GET' && method !== 'HEAD') {
    dropDisallowedHeaders(config)
    return xhrAdapter(config)
  }
  const init: RequestInit = { method, headers: toHeaderRecord(config) }
  if (config.data != null && method !== 'GET' && method !== 'HEAD') {
    init.body = config.data as BodyInit
  }

  // Honour axios's `timeout` and `signal`, which the native fetch would
  // otherwise ignore. A controller rather than AbortSignal.any/timeout, which
  // older iOS webviews lack.
  const controller = new AbortController()
  const timer = config.timeout ? setTimeout(() => controller.abort(), config.timeout) : null
  const callerSignal = config.signal as AbortSignal | undefined
  if (callerSignal?.aborted) controller.abort()
  else callerSignal?.addEventListener?.('abort', () => controller.abort(), { once: true })
  init.signal = controller.signal

  let response: Response
  try {
    response = await tauriFetch(buildUrl(config), init)
  } catch (e) {
    if (controller.signal.aborted && !callerSignal?.aborted) {
      throw new AxiosError(`timeout of ${config.timeout}ms exceeded`, AxiosError.ECONNABORTED, config)
    }
    throw new AxiosError((e as Error).message || 'Network Error', AxiosError.ERR_NETWORK, config)
  } finally {
    if (timer) clearTimeout(timer)
  }

  const headers = new AxiosHeaders()
  response.headers.forEach((value, key) => headers.set(key, value))

  let data: unknown
  switch (config.responseType) {
    case 'blob':
      data = await response.blob()
      break
    case 'arraybuffer':
      data = await response.arrayBuffer()
      break
    case 'text':
      data = await response.text()
      break
    default: {
      const text = await response.text()
      if (!text) data = null
      else {
        try {
          data = JSON.parse(text)
        } catch {
          data = text
        }
      }
    }
  }

  const axiosResponse: AxiosResponse = {
    data,
    status: response.status,
    statusText: response.statusText,
    headers,
    config,
    request: undefined,
  }

  const validate = config.validateStatus
  if (!validate || validate(response.status)) return axiosResponse

  // Reject with the response attached so the 401 -> refresh interceptor fires.
  throw new AxiosError(
    `Request failed with status code ${response.status}`,
    response.status >= 500 ? AxiosError.ERR_BAD_RESPONSE : AxiosError.ERR_BAD_REQUEST,
    config,
    undefined,
    axiosResponse,
  )
}
