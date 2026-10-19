import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const reload = vi.hoisted(() => ({ fn: vi.fn(() => true) }))
vi.mock('@/utils/staleBuild', () => ({ reloadForNewBuild: reload.fn }))

import { respondToOutdatedEditor } from '../outdatedEditor'

beforeEach(() => {
  reload.fn.mockReset()
})
afterEach(() => {
  delete (globalThis as { isTauri?: boolean }).isTauri
})

describe('an editor refused for being outdated', () => {
  it('reloads into the new build on the web, with nothing to show meanwhile', () => {
    reload.fn.mockReturnValue(true)
    expect(respondToOutdatedEditor()).toBeNull()
    expect(reload.fn).toHaveBeenCalledTimes(1)
  })

  it('asks for a reload when one already happened moments ago', () => {
    reload.fn.mockReturnValue(false)
    expect(respondToOutdatedEditor()).toBe('reload')
  })

  it('asks for an app update in the app, without reloading', () => {
    ;(globalThis as { isTauri?: boolean }).isTauri = true
    expect(respondToOutdatedEditor()).toBe('update-app')
    expect(reload.fn).not.toHaveBeenCalled()
  })
})

describe('reloadForNewBuild', () => {
  it('reloads once per window', async () => {
    const { reloadForNewBuild } = await vi.importActual<typeof import('@/utils/staleBuild')>('@/utils/staleBuild')
    const assign = vi.fn()
    const original = window.location
    Object.defineProperty(window, 'location', { value: { ...original, assign, reload: vi.fn() }, configurable: true })
    try {
      sessionStorage.clear()
      expect(reloadForNewBuild('/x')).toBe(true)
      expect(reloadForNewBuild('/x')).toBe(false)
      expect(assign).toHaveBeenCalledTimes(1)
    } finally {
      Object.defineProperty(window, 'location', { value: original, configurable: true })
      sessionStorage.clear()
    }
  })
})
