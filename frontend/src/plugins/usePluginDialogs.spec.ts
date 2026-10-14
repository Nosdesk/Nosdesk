/**
 * The host dialog stack: requests stack in open order, each settles exactly
 * once, and a plugin instance that goes away takes its dialogs with it.
 */
import { afterEach, describe, expect, it } from 'vitest'
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
