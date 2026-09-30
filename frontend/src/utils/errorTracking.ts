/**
 * Browser error reports.
 *
 * Errors nothing else caught (uncaught exceptions, unhandled promise
 * rejections, Vue render and lifecycle errors) and the API 5xx a browser saw
 * are posted to this instance's own server at `/api/client-errors`, which
 * writes them to its log. They go nowhere else.
 *
 * A broken page can't flood the server: each distinct error is sent once per
 * page (repeats raise its count), at most 10 per page and 30 per browser
 * session, in batches, and the endpoint has its own rate limit. Ignored: a tab
 * left open across a deploy (the stale-build reload handles it), aborted and
 * failed-network requests, ResizeObserver notices, opaque cross-origin
 * "Script error." and anything thrown from a browser extension.
 *
 * On in production builds; elsewhere only with
 * `localStorage['nosdesk:error-reports'] = 'on'`.
 */
import type { App } from 'vue'
import axios from 'axios'

import { isStaleChunkError } from './staleBuild'

export type Surface = 'agent' | 'portal' | 'teams' | 'widget'
type Kind = 'error' | 'unhandled_rejection' | 'vue' | 'http'

export interface ErrorReport {
  kind: Kind
  surface: Surface
  message: string
  stack: string
  route: string
  build_sha: string
  status_code?: number
  count: number
}

export interface ErrorTrackingOptions {
  surface: Surface
  /** Where reports go. Read at send time, after the transport is set up. */
  endpoint: () => string
  /** The current route's pattern, not its path: `/tickets/:id`. */
  route: () => string
}

/** What the reporter needs from the page; replaced in tests. */
export interface ReporterEnv {
  enabled: boolean
  buildSha: string
  send: (url: string, body: string, pageHidden: boolean) => void
  sessionCount: { get: () => number; set: (n: number) => void }
  schedule: (fn: () => void, ms: number) => void
}

const PAGE_CAP = 10
const SESSION_CAP = 30
const FLUSH_DELAY_MS = 2000
const MAX_MESSAGE = 300
const MAX_STACK = 4000
const SESSION_KEY = 'nosdesk:error-reports-sent'
const ENABLE_KEY = 'nosdesk:error-reports'

const RESIZE_OBSERVER = /ResizeObserver loop/i
const SCRIPT_ERROR = /^Script error\.?$/i
const NETWORK =
  /^(?:Network Error|Failed to fetch|Load failed|NetworkError when attempting to fetch resource\.?)$/i
const EXTENSION = /(?:chrome|moz|safari(?:-web)?)-extension:\/\//i
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

interface Described {
  name: string
  message: string
  stack: string
}

function describe(error: unknown): Described {
  if (error instanceof Error || (error !== null && typeof error === 'object' && 'message' in error)) {
    const e = error as { name?: unknown; message?: unknown; stack?: unknown }
    const name = typeof e.name === 'string' ? e.name : ''
    const message = typeof e.message === 'string' ? e.message : ''
    return {
      name,
      message: name && name !== 'Error' && !message.startsWith(name) ? `${name}: ${message}` : message,
      stack: typeof e.stack === 'string' ? e.stack : '',
    }
  }
  if (typeof error === 'string') return { name: '', message: error, stack: '' }
  // Never serialise an arbitrary value: a rejected API payload can carry user data.
  const type =
    error !== null && typeof error === 'object'
      ? ((error as object).constructor?.name ?? 'object')
      : error === null
        ? 'null'
        : typeof error
  return { name: '', message: `Non-error value (${type})`, stack: '' }
}

function ignored(error: unknown, d: Described): boolean {
  if (axios.isCancel(error) || d.name === 'AbortError' || d.name === 'CanceledError') return true
  if (isStaleChunkError(error)) return true
  const bare = d.message.replace(/^(?:\w*Error|Uncaught): /, '')
  if (RESIZE_OBSERVER.test(bare) || NETWORK.test(bare)) return true
  if (SCRIPT_ERROR.test(bare) && !d.stack) return true
  return EXTENSION.test(d.stack)
}

/** The first line of a stack that names a position, for telling errors apart. */
function topFrame(stack: string): string {
  return stack.split('\n').find((line) => /:\d+:\d+/.test(line))?.trim() ?? ''
}

/** An API path with IDs and tokens replaced, so one outage groups into one report. */
export function apiPath(url: string | undefined): string {
  if (!url) return '(unknown)'
  const path = url.split(/[?#]/)[0].replace(/^[a-z][a-z0-9+.-]*:\/\/[^/]*/i, '')
  const normalised = path
    .split('/')
    .map((segment) => {
      if (UUID.test(segment)) return ':uuid'
      if (/^\d+$/.test(segment)) return ':id'
      if (segment.length >= 20) return ':token'
      return segment
    })
    .join('/')
  return normalised || '/'
}

export function createReporter(options: ErrorTrackingOptions, env: ReporterEnv) {
  const queue: ErrorReport[] = []
  const queued = new Map<string, ErrorReport>()
  const seen = new Set<string>()
  let sentThisPage = 0
  let scheduled = false

  function flush(pageHidden = false) {
    scheduled = false
    if (queue.length === 0) return
    const reports = queue.splice(0)
    queued.clear()
    // Never throw from here: a throw in the flush timer would itself be an
    // uncaught error, and report itself.
    try {
      env.send(options.endpoint(), JSON.stringify({ reports }), pageHidden)
    } catch {
      // Dropped. The next error tries again.
    }
  }

  function route(): string {
    try {
      return options.route()
    } catch {
      return ''
    }
  }

  function add(key: string, report: Pick<ErrorReport, 'kind' | 'message' | 'stack' | 'status_code'>) {
    if (!env.enabled) return
    const waiting = queued.get(key)
    if (waiting) {
      waiting.count += 1
      return
    }
    if (seen.has(key) || sentThisPage >= PAGE_CAP) return
    const sessionSent = env.sessionCount.get()
    if (sessionSent >= SESSION_CAP) return
    seen.add(key)
    sentThisPage += 1
    env.sessionCount.set(sessionSent + 1)
    const full: ErrorReport = {
      ...report,
      surface: options.surface,
      route: route(),
      build_sha: env.buildSha,
      count: 1,
    }
    queue.push(full)
    queued.set(key, full)
    if (!scheduled) {
      scheduled = true
      env.schedule(() => flush(), FLUSH_DELAY_MS)
    }
  }

  return {
    capture(kind: Exclude<Kind, 'http'>, error: unknown) {
      const d = describe(error)
      if (ignored(error, d)) return
      const message = d.message.slice(0, MAX_MESSAGE)
      const stack = d.stack.slice(0, MAX_STACK)
      add(`${kind}|${message}|${topFrame(stack)}`, { kind, message, stack })
    },
    captureHttp(status: number, method: string | undefined, url: string | undefined) {
      if (status < 500 || status > 599) return
      const message = `${status} ${(method ?? 'get').toUpperCase()} ${apiPath(url)}`
      add(`http|${message}`, { kind: 'http', message, stack: '', status_code: status })
    },
    flush,
  }
}

type Reporter = ReturnType<typeof createReporter>

let active: Reporter | null = null

function browserEnv(): ReporterEnv {
  let enabled = import.meta.env.PROD
  try {
    enabled = enabled || localStorage.getItem(ENABLE_KEY) === 'on'
  } catch {
    // Storage blocked: production builds still report.
  }
  return {
    enabled,
    buildSha: (import.meta.env.VITE_BUILD_SHA as string | undefined) || 'dev',
    send(url, body, pageHidden) {
      // text/plain keeps this a simple request: no CORS preflight from the
      // mobile app's origin, and sendBeacon accepts it as is.
      const type = 'text/plain;charset=UTF-8'
      if (pageHidden && navigator.sendBeacon?.(url, new Blob([body], { type }))) return
      fetch(url, {
        method: 'POST',
        body,
        headers: { 'Content-Type': type },
        keepalive: true,
        credentials: 'omit',
      }).catch(() => {})
    },
    sessionCount: {
      get: () => {
        try {
          return Number(sessionStorage.getItem(SESSION_KEY) ?? 0) || 0
        } catch {
          return 0
        }
      },
      set: (n) => {
        try {
          sessionStorage.setItem(SESSION_KEY, String(n))
        } catch {
          // No storage: the per-page cap still holds.
        }
      },
    },
    schedule: (fn, ms) => {
      setTimeout(fn, ms)
    },
  }
}

/** Start reporting for one app. Call once, after the router is installed. */
export function initErrorTracking(
  app: App,
  options: ErrorTrackingOptions,
  env: ReporterEnv = browserEnv(),
): void {
  const reporter = createReporter(options, env)
  active = reporter
  if (!env.enabled) return

  const previous = app.config.errorHandler
  app.config.errorHandler = (err, instance, info) => {
    reporter.capture('vue', err)
    if (previous) previous(err, instance, info)
    else console.error(err)
  }
  window.addEventListener('error', (event) => {
    // A failed <img> or <script> load is a plain Event, not an ErrorEvent.
    if (event instanceof ErrorEvent) reporter.capture('error', event.error ?? event.message)
  })
  window.addEventListener('unhandledrejection', (event) => {
    reporter.capture('unhandled_rejection', (event as PromiseRejectionEvent).reason)
  })
  window.addEventListener('pagehide', () => reporter.flush(true))
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') reporter.flush(true)
  })
}

export const ErrorTracker = {
  /** An API response that failed with a 5xx, as the browser saw it. */
  captureHttp(status: number, method?: string, url?: string) {
    active?.captureHttp(status, method, url)
  },
}
