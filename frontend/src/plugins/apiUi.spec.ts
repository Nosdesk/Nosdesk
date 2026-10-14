/**
 * `api.ui`: the host dialogs a plugin asks for, and record / page navigation.
 * The plugin gets a minimal projection of what the user picked, null (false
 * for confirm) on cancel, and can open only its own full-page components.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Plugin } from '@nosdesk/core/types/plugin'

const navigateForPlugin = vi.fn()
vi.mock('./pluginNavigation', () => ({ navigateForPlugin }))
// The real store pulls in the router and every view.
vi.mock('@/stores/auth', () => ({ useAuthStore: vi.fn() }))

const { useAuthStore } = await import('@/stores/auth')
const { createPluginAPI } = await import('./api')
const { pluginDialogs, settlePluginDialog } = await import('./usePluginDialogs')

const plugin = {
  uuid: '0190a5b2-0000-7000-8000-000000000003',
  name: 'asset-sublists',
  display_name: 'Asset Sublists',
  // No permissions at all: pickers and navigation need none.
  consented_permissions: [],
  manifest: {
    permissions: [],
    components: {
      page: { slot: 'nav.item' },
      assetsPage: { slot: 'nav.assets.item' },
      panel: { slot: 'asset.info.panel' },
      staffPage: { slot: 'nav.item', audience: 'staff' },
      adminPage: { slot: 'nav.item', audience: 'admin' },
    },
  },
} as unknown as Plugin

const apis: ReturnType<typeof createPluginAPI>[] = []
function api() {
  const a = createPluginAPI(plugin)
  apis.push(a)
  return a
}

/** Let the api's async method body reach `openPluginDialog`. */
async function opened() {
  await Promise.resolve()
  await Promise.resolve()
  return pluginDialogs.value[pluginDialogs.value.length - 1]
}

afterEach(() => {
  for (const a of apis.splice(0)) a._dispose()
  navigateForPlugin.mockClear()
})

describe('plugin ui dialogs', () => {
  it('reports runtime API 1.2', () => {
    expect(api().version).toBe('1.2.0')
  })

  it('confirm opens an attributed request and resolves true on confirm', async () => {
    const p = api().ui.confirm({ title: 'Delete the sublist?', message: 'It has 3 rows.', danger: true })
    const req = await opened()
    expect(req).toMatchObject({
      kind: 'confirm',
      pluginName: 'Asset Sublists',
      options: { title: 'Delete the sublist?', message: 'It has 3 rows.', danger: true },
    })
    settlePluginDialog(req.id, true)
    await expect(p).resolves.toBe(true)
  })

  it('confirm resolves false on dismiss and needs a title', async () => {
    const a = api()
    const p = a.ui.confirm({ title: 'Sure?' })
    settlePluginDialog((await opened()).id)
    await expect(p).resolves.toBe(false)
    await expect(a.ui.confirm({ title: '  ' })).rejects.toMatchObject({ name: 'PluginApiError:invalid' })
  })

  it('pickUser hands back only uuid and name', async () => {
    const p = api().ui.pickUser({ title: 'Owner' })
    const req = await opened()
    expect(req).toMatchObject({ kind: 'user', options: { title: 'Owner' } })
    settlePluginDialog(req.id, {
      uuid: '0190a5b2-0000-7000-8000-0000000000a1',
      name: 'Sam Rivera',
      email: 'sam@example.test',
      role: 'agent',
    })
    await expect(p).resolves.toEqual({ uuid: '0190a5b2-0000-7000-8000-0000000000a1', name: 'Sam Rivera' })
  })

  it('pickAsset hands back id, name and asset tag', async () => {
    const p = api().ui.pickAsset()
    settlePluginDialog((await opened()).id, { id: 9, name: 'MacBook', assetTag: 'IT-0009', serial_number: 'X' })
    await expect(p).resolves.toEqual({ id: 9, name: 'MacBook', assetTag: 'IT-0009' })
  })

  it('pickTicket hands back id and title', async () => {
    const p = api().ui.pickTicket()
    settlePluginDialog((await opened()).id, { id: 31, title: 'Printer jam', number: 12 })
    await expect(p).resolves.toEqual({ id: 31, title: 'Printer jam' })
  })

  it('every picker resolves null on cancel', async () => {
    // A fresh instance each: four dismissals in a row on one would trip the
    // dismissal-loop guard.
    const picks = ['pickUser', 'pickAsset', 'pickTicket', 'pickDate'] as const
    for (const pick of picks) {
      const p = api().ui[pick]()
      settlePluginDialog((await opened()).id)
      await expect(p).resolves.toBeNull()
    }
  })

  it('refuses dialogs after the user dismisses three in a row', async () => {
    const a = api()
    for (let i = 0; i < 3; i++) {
      const p = a.ui.pickUser()
      settlePluginDialog((await opened()).id)
      await expect(p).resolves.toBeNull()
    }
    await expect(a.ui.confirm({ title: 'Again?' })).rejects.toMatchObject({ code: 'denied' })
    // A remounted instance starts clean.
    const p = api().ui.pickUser()
    settlePluginDialog((await opened()).id, { uuid: 'u', name: 'Sam' })
    await expect(p).resolves.toEqual({ uuid: 'u', name: 'Sam' })
  })

  it('pickDate takes a date-only initial and resolves YYYY-MM-DD', async () => {
    const a = api()
    const p = a.ui.pickDate({ initial: '2026-10-14' })
    const req = await opened()
    expect(req).toMatchObject({ kind: 'date', options: { initial: '2026-10-14' } })
    settlePluginDialog(req.id, '2026-11-02')
    await expect(p).resolves.toBe('2026-11-02')
    await expect(a.ui.pickDate({ initial: '2026-02-30' })).rejects.toMatchObject({ name: 'PluginApiError:invalid' })
    await expect(a.ui.pickDate({ initial: '2026-10-14T00:00:00Z' })).rejects.toMatchObject({
      name: 'PluginApiError:invalid',
    })
  })

  it('a disposed instance settles its open dialog and refuses new ones', async () => {
    const a = api()
    const p = a.ui.pickUser()
    await opened()
    a._dispose()
    await expect(p).resolves.toBeNull()
    expect(pluginDialogs.value).toHaveLength(0)
    await expect(a.ui.confirm({ title: 'Sure?' })).rejects.toMatchObject({ name: 'PluginApiError:invalid' })
  })
})

describe('plugin ui navigation', () => {
  it('opens each record type through the app routes', async () => {
    const a = api()
    await a.ui.openRecord({ type: 'ticket', id: 31 })
    await a.ui.openRecord({ type: 'asset', id: 9 })
    await a.ui.openRecord({ type: 'user', id: '0190a5b2-0000-7000-8000-0000000000a1' })
    expect(navigateForPlugin.mock.calls.map((c) => c[0])).toEqual([
      { type: 'ticket', id: 31 },
      { type: 'asset', id: 9 },
      { type: 'user', uuid: '0190a5b2-0000-7000-8000-0000000000a1' },
    ])
  })

  it('refuses a malformed record reference', async () => {
    const a = api()
    for (const ref of [
      { type: 'ticket', id: '31' },
      { type: 'asset', id: -1 },
      { type: 'asset', id: 1.5 },
      { type: 'user', id: '../admin' },
      { type: 'document', id: 1 },
    ]) {
      await expect(a.ui.openRecord(ref as never)).rejects.toMatchObject({ name: 'PluginApiError:invalid' })
    }
    expect(navigateForPlugin).not.toHaveBeenCalled()
  })

  it('opens the plugin\'s own nav.item and nav.assets.item pages', async () => {
    const a = api()
    await a.ui.openPage('page')
    await a.ui.openPage('assetsPage')
    expect(navigateForPlugin.mock.calls.map((c) => c[0])).toEqual([
      { type: 'page', pluginUuid: plugin.uuid, componentName: 'page' },
      { type: 'page', pluginUuid: plugin.uuid, componentName: 'assetsPage' },
    ])
  })

  it('refuses a component that is not one of its full-page components', async () => {
    const a = api()
    for (const name of ['panel', 'nope', 'toString', '__proto__']) {
      await expect(a.ui.openPage(name)).rejects.toMatchObject({ name: 'PluginApiError:invalid' })
    }
    expect(navigateForPlugin).not.toHaveBeenCalled()
  })

  it('refuses a page above the viewer\'s audience exactly like a missing one', async () => {
    vi.mocked(useAuthStore).mockReturnValue({ isTechnician: true, isAdmin: false } as never)
    const a = api()
    const hidden = await a.ui.openPage('adminPage').catch((e: unknown) => e)
    const missing = await a.ui.openPage('nope').catch((e: unknown) => e)
    expect(hidden).toMatchObject({ name: 'PluginApiError:invalid' })
    expect((hidden as Error).message).toBe((missing as Error).message)
    expect(navigateForPlugin).not.toHaveBeenCalled()

    await a.ui.openPage('staffPage')
    vi.mocked(useAuthStore).mockReturnValue({ isTechnician: true, isAdmin: true } as never)
    await a.ui.openPage('adminPage')
    expect(navigateForPlugin.mock.calls.map((c) => c[0].componentName)).toEqual(['staffPage', 'adminPage'])
    vi.mocked(useAuthStore).mockReset()
  })
})
