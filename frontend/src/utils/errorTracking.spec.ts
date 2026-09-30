import { describe, expect, it, vi } from 'vitest'
import { createApp } from 'vue'
import axios from 'axios'

import {
  apiPath,
  createReporter,
  initErrorTracking,
  type ErrorReport,
  type ErrorTrackingOptions,
  type ReporterEnv,
} from './errorTracking'

function setup(overrides: Partial<ReporterEnv> = {}, sessionStart = 0) {
  let session = sessionStart
  const timers: Array<() => void> = []
  const send = vi.fn<(url: string, body: string, pageHidden: boolean) => void>()
  const env: ReporterEnv = {
    enabled: true,
    buildSha: 'abc1234',
    send,
    sessionCount: { get: () => session, set: (n) => (session = n) },
    schedule: (fn) => timers.push(fn),
    ...overrides,
  }
  const options: ErrorTrackingOptions = {
    surface: 'agent',
    endpoint: () => '/api/client-errors',
    route: () => '/tickets/:id',
  }
  const reporter = createReporter(options, env)
  const sent = (): ErrorReport[] =>
    send.mock.calls.flatMap(([, body]) => (JSON.parse(body) as { reports: ErrorReport[] }).reports)
  return { reporter, send, sent, timers, env, options, session: () => session }
}

describe('error reports', () => {
  it('batches a report with its route, build and a stack, and sends it once', () => {
    const { reporter, send, sent, timers } = setup()
    reporter.capture('error', new TypeError('boom'))
    expect(send).not.toHaveBeenCalled()
    expect(timers).toHaveLength(1)
    timers[0]()
    expect(send).toHaveBeenCalledTimes(1)
    expect(send.mock.calls[0][0]).toBe('/api/client-errors')
    const [report] = sent()
    expect(report).toMatchObject({
      kind: 'error',
      surface: 'agent',
      message: 'TypeError: boom',
      route: '/tickets/:id',
      build_sha: 'abc1234',
      count: 1,
    })
    expect(report.stack).toContain('boom')
  })

  it('counts repeats while queued and never resends an error it already sent', () => {
    const { reporter, sent, timers } = setup()
    const err = new Error('same')
    reporter.capture('vue', err)
    reporter.capture('vue', err)
    reporter.capture('vue', err)
    timers[0]()
    reporter.capture('vue', err)
    reporter.flush()
    expect(sent()).toHaveLength(1)
    expect(sent()[0].count).toBe(3)
  })

  it('stops at ten distinct errors a page and thirty a session', () => {
    const page = setup()
    for (let i = 0; i < 15; i++) page.reporter.capture('error', new Error(`e${i}`))
    page.reporter.flush()
    expect(page.sent()).toHaveLength(10)

    const session = setup({}, 29)
    session.reporter.capture('error', new Error('one'))
    session.reporter.capture('error', new Error('two'))
    session.reporter.flush()
    expect(session.sent()).toHaveLength(1)
    expect(session.session()).toBe(30)
  })

  it('ignores stale chunks, aborts, network failures and browser noise', () => {
    const { reporter, sent } = setup()
    const extension = new Error('from an extension')
    extension.stack = 'Error: x\n    at f (chrome-extension://abcdef/content.js:1:2)'
    const abort = new Error('The user aborted a request.')
    abort.name = 'AbortError'
    const axiosNetwork = Object.assign(new Error('Network Error'), { name: 'AxiosError' })
    const scriptError = new Error('Script error.')
    scriptError.stack = ''
    for (const error of [
      new TypeError('Failed to fetch dynamically imported module: https://app/static/X-abc.js'),
      new axios.CanceledError(),
      abort,
      new TypeError('Failed to fetch'),
      new TypeError('Load failed'),
      axiosNetwork,
      'ResizeObserver loop completed with undelivered notifications.',
      scriptError,
      extension,
    ]) {
      reporter.capture('error', error)
    }
    reporter.flush()
    expect(sent()).toHaveLength(0)
  })

  it('never serialises an arbitrary rejected value', () => {
    const { reporter, sent } = setup()
    reporter.capture('unhandled_rejection', { email: 'someone@example.com', token: 'secret' })
    reporter.capture('unhandled_rejection', 'plain string reason')
    reporter.capture('unhandled_rejection', undefined)
    reporter.flush()
    const messages = sent().map((r) => r.message)
    expect(messages).toEqual(['Non-error value (Object)', 'plain string reason', 'Non-error value (undefined)'])
    expect(JSON.stringify(sent())).not.toContain('someone@example.com')
  })

  it('reports API 5xx once per path shape, without IDs or tokens', () => {
    const { reporter, sent } = setup()
    reporter.captureHttp(502, 'get', '/tickets/123/comments?x=1')
    reporter.captureHttp(502, 'get', '/tickets/456/comments')
    reporter.captureHttp(404, 'get', '/tickets/1')
    reporter.captureHttp(500, 'post', 'https://api.example.test/api/auth/invitation/abcdefghijklmnopqrstuvwxyz')
    reporter.flush()
    expect(sent()).toEqual([
      expect.objectContaining({ kind: 'http', status_code: 502, message: '502 GET /tickets/:id/comments', count: 2 }),
      expect.objectContaining({ kind: 'http', status_code: 500, message: '500 POST /api/auth/invitation/:token' }),
    ])
  })

  it('normalises API paths', () => {
    expect(apiPath('/workspaces/0b8e6c1e-8a5a-4f5c-9d61-0a1b2c3d4e5f/members')).toBe('/workspaces/:uuid/members')
    expect(apiPath(undefined)).toBe('(unknown)')
    expect(apiPath('')).toBe('(unknown)')
  })

  it('sends nothing when disabled, and survives a route or endpoint that throws', () => {
    const off = setup({ enabled: false })
    off.reporter.capture('error', new Error('x'))
    off.reporter.flush()
    expect(off.send).not.toHaveBeenCalled()

    const { reporter, sent, options, send } = setup()
    options.route = () => {
      throw new Error('router not ready')
    }
    reporter.capture('error', new Error('early'))
    reporter.flush()
    expect(sent()[0].route).toBe('')

    options.endpoint = () => {
      throw new Error('transport not configured')
    }
    reporter.capture('error', new Error('later'))
    expect(() => reporter.flush()).not.toThrow()
    expect(send).toHaveBeenCalledTimes(1)
  })

  it('hooks Vue, uncaught errors and rejections, and keeps an existing Vue handler', () => {
    const send = vi.fn<(url: string, body: string, pageHidden: boolean) => void>()
    const env: ReporterEnv = {
      enabled: true,
      buildSha: 'dev',
      send,
      sessionCount: { get: () => 0, set: () => {} },
      schedule: () => {},
    }
    const app = createApp({})
    const previous = vi.fn()
    app.config.errorHandler = previous
    initErrorTracking(app, { surface: 'portal', endpoint: () => '/api/client-errors', route: () => '' }, env)

    app.config.errorHandler?.(new Error('render'), null, 'render function')
    expect(previous).toHaveBeenCalledTimes(1)
    window.dispatchEvent(new ErrorEvent('error', { error: new RangeError('uncaught'), message: 'Uncaught RangeError: uncaught' }))
    window.dispatchEvent(Object.assign(new Event('unhandledrejection'), { reason: new Error('rejected') }))
    window.dispatchEvent(new Event('pagehide'))

    expect(send).toHaveBeenCalledTimes(1)
    const [url, body, pageHidden] = send.mock.calls[0]
    expect(url).toBe('/api/client-errors')
    expect(pageHidden).toBe(true)
    const kinds = (JSON.parse(body) as { reports: ErrorReport[] }).reports.map((r) => [r.kind, r.surface, r.message])
    expect(kinds).toEqual([
      ['vue', 'portal', 'render'],
      ['error', 'portal', 'RangeError: uncaught'],
      ['unhandled_rejection', 'portal', 'rejected'],
    ])
  })
})
