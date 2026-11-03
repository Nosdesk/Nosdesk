/**
 * `api.ui.openExternal`: https only, the host covered by a `link:` grant, and
 * opened by the host. When the browser would block the tab, the host asks in a
 * dialog and opens the link from the user's own click.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Plugin } from '@nosdesk/core/types/plugin'

const platform = vi.hoisted(() => ({
  isTauriRuntime: vi.fn(() => false),
  openExternalUrl: vi.fn(async (_url: string) => {}),
}))
vi.mock('@/platform', () => platform)
// The real store pulls in the router and every view.
vi.mock('@/stores/auth', () => ({ useAuthStore: vi.fn() }))

const { createPluginAPI } = await import('./api')
const { pluginDialogs, settlePluginDialog } = await import('./usePluginDialogs')

function pluginWith(permissions: string[]): Plugin {
  return {
    uuid: '0190a5b2-0000-7000-8000-000000000004',
    name: 'github-integration',
    display_name: 'GitHub',
    consented_permissions: permissions,
    manifest: { permissions, components: {} },
  } as unknown as Plugin
}

const apis: ReturnType<typeof createPluginAPI>[] = []
function api(permissions: string[] = ['link:github.com']) {
  const a = createPluginAPI(pluginWith(permissions))
  apis.push(a)
  return a
}

function setActivation(isActive: boolean | undefined) {
  Object.defineProperty(navigator, 'userActivation', {
    configurable: true,
    value: isActive === undefined ? undefined : { isActive, hasBeenActive: isActive },
  })
}

beforeEach(() => setActivation(true))
afterEach(() => {
  for (const a of apis.splice(0)) a._dispose()
  vi.clearAllMocks()
  platform.isTauriRuntime.mockReturnValue(false)
  setActivation(undefined)
})

describe('ui.openExternal', () => {
  it('opens a granted https URL through the host', async () => {
    await api().ui.openExternal('https://github.com/nosdesk/plugin-github/issues/12')
    expect(platform.openExternalUrl).toHaveBeenCalledWith(
      'https://github.com/nosdesk/plugin-github/issues/12',
    )
  })

  it('needs a link grant covering the host, not a network grant', async () => {
    const networkOnly = api(['network:api.github.com', 'network:github.com'])
    await expect(networkOnly.ui.openExternal('https://github.com/x')).rejects.toMatchObject({
      name: 'PluginApiError:denied',
      message: expect.stringContaining('link:github.com not granted'),
    })
    await expect(api().ui.openExternal('https://gist.github.com/x')).rejects.toMatchObject({
      name: 'PluginApiError:denied',
    })
    expect(platform.openExternalUrl).not.toHaveBeenCalled()
  })

  it('covers one subdomain level with a wildcard grant', async () => {
    const a = api(['link:*.github.com'])
    await a.ui.openExternal('https://github.com/a')
    await a.ui.openExternal('https://gist.github.com/b')
    await expect(a.ui.openExternal('https://a.b.github.com/c')).rejects.toMatchObject({
      name: 'PluginApiError:denied',
    })
    expect(platform.openExternalUrl).toHaveBeenCalledTimes(2)
  })

  it('refuses anything but a plain https URL', async () => {
    const a = api()
    for (const url of [
      'http://github.com/x',
      'javascript:alert(1)',
      'not a url',
      'https://user:pw@github.com/x',
      'https://user@github.com/x',
    ]) {
      await expect(a.ui.openExternal(url)).rejects.toMatchObject({ name: 'PluginApiError:invalid' })
    }
    expect(platform.openExternalUrl).not.toHaveBeenCalled()
  })

  it('asks first when the browser would block the tab, and opens on the user click', async () => {
    setActivation(false)
    const a = api()
    const p = a.ui.openExternal('https://github.com/o/r/pull/7')
    await Promise.resolve()
    await Promise.resolve()
    const req = pluginDialogs.value[pluginDialogs.value.length - 1]
    expect(req).toMatchObject({
      kind: 'link',
      pluginName: 'GitHub',
      options: { url: 'https://github.com/o/r/pull/7', host: 'github.com' },
    })
    expect(platform.openExternalUrl).not.toHaveBeenCalled()
    settlePluginDialog(req.id, true)
    await expect(p).resolves.toBeUndefined()
  })

  it('opens straight away in the app, where nothing blocks the tab', async () => {
    setActivation(false)
    platform.isTauriRuntime.mockReturnValue(true)
    await api().ui.openExternal('https://github.com/o/r')
    expect(platform.openExternalUrl).toHaveBeenCalledWith('https://github.com/o/r')
    expect(pluginDialogs.value).toHaveLength(0)
  })
})
