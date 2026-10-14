/**
 * The sandbox host API forwards `ui.*` and `assets.groups` to the in-process
 * impl, and the dialog calls run on the governor's interactive lane (no 15s
 * timeout).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Plugin } from '@nosdesk/core/types/plugin'

const inprocUi = {
  isPrinting: vi.fn(() => false),
  confirm: vi.fn(),
  pickUser: vi.fn(),
  pickAsset: vi.fn(),
  pickTicket: vi.fn(),
  pickDate: vi.fn(),
  openRecord: vi.fn(async () => undefined),
  openPage: vi.fn(async () => undefined),
}
const inprocAssets = {
  groups: vi.fn(),
}
vi.mock('./api', () => ({
  createPluginAPI: () => ({
    version: '1.2.0',
    plugin: { uuid: 'u', name: 'n', displayName: 'N', version: '1' },
    assets: inprocAssets,
    ui: inprocUi,
  }),
}))

const { createHostApiImpl } = await import('./sandboxHostApi')

describe('sandbox host api ui forwarding', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    vi.useRealTimers()
    vi.clearAllMocks()
  })

  it('forwards each ui method with its arguments and result', async () => {
    inprocUi.confirm.mockResolvedValue(true)
    inprocUi.pickUser.mockResolvedValue({ uuid: 'x', name: 'Sam' })
    inprocUi.pickAsset.mockResolvedValue({ id: 1, name: 'Mac', assetTag: null })
    inprocUi.pickTicket.mockResolvedValue({ id: 2, title: 'Jam' })
    inprocUi.pickDate.mockResolvedValue('2026-10-14')
    const { hostApi } = createHostApiImpl({} as Plugin)

    await expect(hostApi.ui.confirm({ title: 'Sure?' })).resolves.toBe(true)
    expect(inprocUi.confirm).toHaveBeenCalledWith({ title: 'Sure?' })
    await expect(hostApi.ui.pickUser({ title: 'Owner' })).resolves.toEqual({ uuid: 'x', name: 'Sam' })
    expect(inprocUi.pickUser).toHaveBeenCalledWith({ title: 'Owner' })
    await expect(hostApi.ui.pickAsset()).resolves.toEqual({ id: 1, name: 'Mac', assetTag: null })
    await expect(hostApi.ui.pickTicket()).resolves.toEqual({ id: 2, title: 'Jam' })
    await expect(hostApi.ui.pickDate({ initial: '2026-10-01' })).resolves.toBe('2026-10-14')
    expect(inprocUi.pickDate).toHaveBeenCalledWith({ initial: '2026-10-01' })
    await hostApi.ui.openRecord({ type: 'asset', id: 1 })
    expect(inprocUi.openRecord).toHaveBeenCalledWith({ type: 'asset', id: 1 })
    await hostApi.ui.openPage('page')
    expect(inprocUi.openPage).toHaveBeenCalledWith('page')
    await expect(hostApi.ui.isPrinting()).resolves.toBe(false)
  })

  it('lets a dialog stay open past the bridge call timeout, one at a time', async () => {
    let answer!: (v: boolean) => void
    inprocUi.confirm.mockReturnValue(new Promise<boolean>((r) => (answer = r)))
    const { hostApi } = createHostApiImpl({} as Plugin)

    let result: boolean | string | undefined
    hostApi.ui.confirm({ title: 'Sure?' }).then(
      (v) => (result = v),
      (e: Error) => (result = e.name),
    )
    await vi.advanceTimersByTimeAsync(20_000)
    expect(result).toBeUndefined()
    await expect(hostApi.ui.pickUser()).rejects.toMatchObject({ name: 'PluginApiError:rate_limited' })

    answer(true)
    await vi.advanceTimersByTimeAsync(0)
    expect(result).toBe(true)
  })
})

describe('sandbox host api asset forwarding', () => {
  afterEach(() => vi.clearAllMocks())

  it('forwards assets.groups and its result', async () => {
    const groups = [{ id: 4, name: 'Lab', color: '#3b82f6' }]
    inprocAssets.groups.mockResolvedValue(groups)
    const { hostApi } = createHostApiImpl({} as Plugin)
    await expect(hostApi.assets.groups()).resolves.toEqual(groups)
    expect(inprocAssets.groups).toHaveBeenCalledOnce()
  })
})
