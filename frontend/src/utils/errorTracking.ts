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
/** Stack lines kept, and the length of each: the frames the server reads, no more. */
const MAX_FRAMES = 8
const MAX_FRAME_LINE = 200
/** One request's body, in bytes. The server refuses more than 32 KB. */
const MAX_BODY_BYTES = 24 * 1024
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

/**
 * The stack's frame lines only, trimmed. The message line V8 puts first can be
 * long and is already in `message`; ten untrimmed stacks would overflow the
 * request.
 */
function frameLines(stack: string): string {
  return stack
    .split('\n')
    .filter((line) => /:\d+:\d+\)?\s*$/.test(line))
    .slice(0, MAX_FRAMES)
    .map((line) => line.slice(0, MAX_FRAME_LINE))
    .join('\n')
}

/** The first line of a stack that names a position, for telling errors apart. */
function topFrame(stack: string): string {
  return stack.split('\n').find((line) => /:\d+:\d+/.test(line))?.trim() ?? ''
}

/**
 * A segment kept as is: lowercase words of up to 16 letters joined by `-` or
 * `_`, as every literal API segment is, or an API version. A long unbroken run
 * of letters is more likely a token than a word.
 */
const WORD_SEGMENT = /^(?:[a-z]{1,16}(?:[-_][a-z]{1,16}){0,5}|v\d{1,2})$/

/**
 * An API path with every segment that is not a lowercase word replaced, so one
 * outage groups into one report and no ID, token, email, file name or slug in a
 * path is sent. The server groups the path again the same way.
 */
export function apiPath(url: string | undefined): string {
  if (!url) return '(unknown)'
  const path = url.split(/[?#]/)[0].replace(/^[a-z][a-z0-9+.-]*:\/\/[^/]*/i, '')
  const segments = path.split('/')
  const normalised = segments.map((segment, i) => groupSegment(segment, segments[i - 1])).join('/')
  return normalised.startsWith('/') ? normalised : `/${normalised}`
}

function groupSegment(segment: string, previous: string | undefined): string {
  if (!segment) return segment
  if (previous === 'slug') return ':slug'
  if (WORD_SEGMENT.test(segment)) return segment
  if (UUID.test(segment)) return ':uuid'
  if (/^\d+$/.test(segment)) return ':id'
  return segment.length >= 20 ? ':token' : ':param'
}

function byteLength(text: string): number {
  return new TextEncoder().encode(text).length
}

export function createReporter(options: ErrorTrackingOptions, env: ReporterEnv) {
  const queue: ErrorReport[] = []
  const queued = new Map<string, ErrorReport>()
  const seen = new Set<string>()
  let sentThisPage = 0
  let scheduled = false

  function send(reports: ErrorReport[], pageHidden: boolean) {
    // Never throw from here: a throw in the flush timer would itself be an
    // uncaught error, and report itself.
    try {
      env.send(options.endpoint(), JSON.stringify({ reports }), pageHidden)
    } catch {
      // Dropped. The next error tries again.
    }
  }

  function flush(pageHidden = false) {
    scheduled = false
    if (queue.length === 0) return
    const reports = queue.splice(0)
    queued.clear()
    // Split so no request passes the server's body cap.
    let batch: ErrorReport[] = []
    for (const report of reports) {
      if (batch.length > 0 && byteLength(JSON.stringify({ reports: [...batch, report] })) > MAX_BODY_BYTES) {
        send(batch, pageHidden)
        batch = []
      }
      batch.push(report)
    }
    send(batch, pageHidden)
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
      const stack = frameLines(d.stack)
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
