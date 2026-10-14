/**
 * The host dialog stack: requests stack in open order, each settles exactly
 * once, and a plugin instance that goes away takes its dialogs with it.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  createDialogOwner,
  openPluginDialog,
  pluginDialogs,
  releaseDialogOwner,
  settlePluginDialog,
  type PluginDialogOwner,
} from './usePluginDialogs'

const owners: PluginDialogOwner[] = []
function owner() {
  const o = createDialogOwner()
  owners.push(o)
  return o
}

afterEach(() => {
  for (const o of owners.splice(0)) releaseDialogOwner(o)
})

describe('plugin dialog stack', () => {
  it('stacks requests in open order, across owners', () => {
    const a = owner()
    const b = owner()
    void openPluginDialog(a, 'confirm', 'Alpha', { title: 'Sure?' })
    void openPluginDialog(b, 'user', 'Beta', {})
    void openPluginDialog(a, 'date', 'Alpha', { initial: '2026-10-14' })
    expect(pluginDialogs.value.map((r) => [r.kind, r.pluginName])).toEqual([
      ['confirm', 'Alpha'],
      ['user', 'Beta'],
      ['date', 'Alpha'],
    ])
  })

  it('settles with the answer, once, and closes', async () => {
    const o = owner()
    const p = openPluginDialog(o, 'ticket', 'Alpha', {})
    const id = pluginDialogs.value[0].id
    settlePluginDialog(id, { id: 4, title: 'Printer' })
    // The reused pickers emit their pick and then `close`.
    settlePluginDialog(id)
    await expect(p).resolves.toEqual({ id: 4, title: 'Printer' })
    expect(pluginDialogs.value).toHaveLength(0)
  })

  it('settles a cancelled dialog as false for confirm and null for pickers', async () => {
    const o = owner()
    const confirm = openPluginDialog(o, 'confirm', 'Alpha', { title: 'Sure?' })
    const pick = openPluginDialog(o, 'asset', 'Alpha', {})
    const [c, a] = pluginDialogs.value
    settlePluginDialog(a.id)
    settlePluginDialog(c.id)
    await expect(confirm).resolves.toBe(false)
    await expect(pick).resolves.toBeNull()
  })

  it('closes only the released owner\'s dialogs, as cancelled', async () => {
    const a = owner()
    const b = owner()
    const aConfirm = openPluginDialog(a, 'confirm', 'Alpha', { title: 'Sure?' })
    const aDate = openPluginDialog(a, 'date', 'Alpha', {})
    void openPluginDialog(b, 'user', 'Beta', {})
    releaseDialogOwner(a)
    await expect(aConfirm).resolves.toBe(false)
    await expect(aDate).resolves.toBeNull()
    expect(pluginDialogs.value.map((r) => r.pluginName)).toEqual(['Beta'])
  })

  it('refuses a dialog for an owner that is no longer mounted', async () => {
    const o = owner()
    releaseDialogOwner(o)
    await expect(openPluginDialog(o, 'user', 'Alpha', {})).rejects.toMatchObject({
      name: 'PluginApiError:invalid',
    })
    expect(pluginDialogs.value).toHaveLength(0)
  })
})

describe('plugin dialog dismissal loop', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  /** Open a dialog and dismiss it, as Cancel / Escape / close would. */
  async function dismiss(o: PluginDialogOwner, kind: 'confirm' | 'user' = 'user') {
    const p = openPluginDialog(o, kind, 'Alpha', kind === 'confirm' ? { title: 'Sure?' } : {})
    settlePluginDialog(pluginDialogs.value[pluginDialogs.value.length - 1].id)
    await p
  }

  const refused = {
    code: 'denied',
    message: expect.stringContaining("the user dismissed this plugin's dialogs repeatedly"),
  }

  it('refuses the fourth dialog after three dismissals within 30 seconds', async () => {
    const o = owner()
    await dismiss(o)
    vi.advanceTimersByTime(10_000)
    await dismiss(o, 'confirm')
    vi.advanceTimersByTime(10_000)
    await dismiss(o)
    await expect(openPluginDialog(o, 'date', 'Alpha', {})).rejects.toMatchObject(refused)
    expect(pluginDialogs.value).toHaveLength(0)
  })

  it('blocks only the owner the user kept dismissing', async () => {
    const a = owner()
    const b = owner()
    for (let i = 0; i < 3; i++) await dismiss(a)
    await expect(openPluginDialog(a, 'user', 'Alpha', {})).rejects.toMatchObject(refused)
    void openPluginDialog(b, 'user', 'Beta', {})
    expect(pluginDialogs.value.map((r) => r.pluginName)).toEqual(['Beta'])
  })

  it('lifts the block after 60 seconds', async () => {
    const o = owner()
    for (let i = 0; i < 3; i++) await dismiss(o)
    vi.advanceTimersByTime(59_999)
    await expect(openPluginDialog(o, 'user', 'Alpha', {})).rejects.toMatchObject(refused)
    vi.advanceTimersByTime(1)
    void openPluginDialog(o, 'user', 'Alpha', {})
    expect(pluginDialogs.value).toHaveLength(1)
  })

  it('does not count dismissals spread over more than 30 seconds', async () => {
    const o = owner()
    await dismiss(o)
    vi.advanceTimersByTime(20_000)
    await dismiss(o)
    vi.advanceTimersByTime(15_000)
    await dismiss(o)
    void openPluginDialog(o, 'user', 'Alpha', {})
    expect(pluginDialogs.value).toHaveLength(1)
  })

  it('resets the count on a pick or a confirm', async () => {
    const o = owner()
    for (const answer of [{ uuid: 'u', name: 'Sam' }, true] as const) {
      await dismiss(o)
      await dismiss(o)
      const p = openPluginDialog(o, answer === true ? 'confirm' : 'user', 'Alpha', { title: 'Sure?' })
      settlePluginDialog(pluginDialogs.value[0].id, answer)
      await p
    }
    await dismiss(o)
    await dismiss(o)
    void openPluginDialog(o, 'user', 'Alpha', {})
    expect(pluginDialogs.value).toHaveLength(1)
  })

  it('does not count dialogs closed by an unmount', async () => {
    const o = owner()
    await dismiss(o)
    await dismiss(o)
    void openPluginDialog(o, 'user', 'Alpha', {})
    releaseDialogOwner(o)
    // A remount brings a new owner with a clean count.
    const next = owner()
    await dismiss(next)
    await dismiss(next)
    void openPluginDialog(next, 'user', 'Alpha', {})
    expect(pluginDialogs.value).toHaveLength(1)
  })
})
