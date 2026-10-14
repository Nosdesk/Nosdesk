/**
 * The bridge governor's interactive lane: host dialogs wait on a person, so
 * they must not time out or hold an in-flight slot, but they still count
 * against the rate window and only one may be open per plugin instance.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  BridgeGovernor,
  INTERACTIVE_HOST_METHODS,
  governHostApi,
  type GovernorOptions,
} from '@nosdesk/plugin-sdk'

const OPTS: GovernorOptions = {
  maxCallsPerWindow: 60,
  windowMs: 1000,
  maxInFlight: 8,
  callTimeoutMs: 15_000,
  maxInteractivePending: 1,
}

function deferred<T>() {
  let resolve!: (v: T) => void
  const promise = new Promise<T>((r) => (resolve = r))
  return { promise, resolve }
}

describe('BridgeGovernor interactive lane', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('does not time out a pending interactive call', async () => {
    const g = new BridgeGovernor(OPTS)
    const answer = deferred<boolean>()
    let settled: boolean | 'error' | undefined
    g.runInteractive(() => answer.promise).then(
      (v) => (settled = v),
      () => (settled = 'error'),
    )
    await vi.advanceTimersByTimeAsync(60_000)
    expect(settled).toBeUndefined()
    answer.resolve(true)
    await vi.advanceTimersByTimeAsync(0)
    expect(settled).toBe(true)
  })

  it('still times out a normal call', async () => {
    const g = new BridgeGovernor(OPTS)
    const p = g.run(() => new Promise(() => {}))
    const caught = p.catch((e: Error) => e.name)
    await vi.advanceTimersByTimeAsync(15_001)
    expect(await caught).toBe('PluginApiError:timeout')
  })

  it('holds no in-flight slot: eight normal calls still run beside it', async () => {
    const g = new BridgeGovernor(OPTS)
    void g.runInteractive(() => new Promise(() => {}))
    const normal = Array.from({ length: 8 }, () => g.run(async () => 'ok'))
    await expect(Promise.all(normal)).resolves.toHaveLength(8)
  })

  it('refuses a second interactive call while one is pending, then admits after', async () => {
    const g = new BridgeGovernor(OPTS)
    const first = deferred<string>()
    const p1 = g.runInteractive(() => first.promise)
    await expect(g.runInteractive(async () => 'second')).rejects.toMatchObject({
      name: 'PluginApiError:rate_limited',
    })
    first.resolve('first')
    await expect(p1).resolves.toBe('first')
    await expect(g.runInteractive(async () => 'third')).resolves.toBe('third')
  })

  it('frees the slot when the interactive call rejects', async () => {
    const g = new BridgeGovernor(OPTS)
    await expect(g.runInteractive(() => Promise.reject(new Error('boom')))).rejects.toThrow('boom')
    await expect(g.runInteractive(async () => 'ok')).resolves.toBe('ok')
  })

  it('counts interactive calls in the rate window', async () => {
    const g = new BridgeGovernor({ ...OPTS, maxCallsPerWindow: 3 })
    await g.runInteractive(async () => 1)
    await g.run(async () => 2)
    await g.runInteractive(async () => 3)
    await expect(g.runInteractive(async () => 4)).rejects.toMatchObject({
      name: 'PluginApiError:rate_limited',
    })
    await expect(g.run(async () => 5)).rejects.toMatchObject({ name: 'PluginApiError:rate_limited' })
    await vi.advanceTimersByTimeAsync(1001)
    await expect(g.runInteractive(async () => 6)).resolves.toBe(6)
  })
})

describe('governHostApi routing', () => {
  it('sends the ui dialog methods to the interactive lane and the rest to run', async () => {
    const g = new BridgeGovernor(OPTS)
    const runSpy = vi.spyOn(g, 'run')
    const interactiveSpy = vi.spyOn(g, 'runInteractive')
    const api = governHostApi(
      {
        notify: async () => undefined,
        ui: {
          isPrinting: async () => false,
          confirm: async () => true,
          pickUser: async () => null,
          pickAsset: async () => null,
          pickTicket: async () => null,
          pickDate: async () => null,
          openRecord: async () => undefined,
          openPage: async () => undefined,
        },
        tickets: { confirm: async () => 'not a dialog' },
      },
      g,
    )
    await api.ui.confirm()
    await api.ui.pickUser()
    await api.ui.pickAsset()
    await api.ui.pickTicket()
    await api.ui.pickDate()
    expect(interactiveSpy).toHaveBeenCalledTimes(5)

    await api.ui.isPrinting()
    await api.ui.openRecord()
    await api.ui.openPage()
    await api.notify()
    // Matched by full path: a same-named method elsewhere is a normal call.
    await api.tickets.confirm()
    expect(runSpy).toHaveBeenCalledTimes(5)
    expect(interactiveSpy).toHaveBeenCalledTimes(5)
  })

  it('lists exactly the dialog methods as interactive', () => {
    expect([...INTERACTIVE_HOST_METHODS].sort()).toEqual([
      'ui.confirm',
      'ui.pickAsset',
      'ui.pickDate',
      'ui.pickTicket',
      'ui.pickUser',
    ])
  })
})
