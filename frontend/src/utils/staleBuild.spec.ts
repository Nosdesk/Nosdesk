import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { isStaleChunkError, reloadForNewBuild } from './staleBuild'

describe('stale build recovery', () => {
  const assign = vi.fn()
  beforeEach(() => {
    sessionStorage.clear()
    assign.mockClear()
    Object.defineProperty(window, 'location', {
      value: { ...window.location, assign, reload: vi.fn() },
      writable: true,
    })
  })
  afterEach(() => vi.useRealTimers())

  it('recognises the browsers’ missing-chunk errors, and nothing else', () => {
    expect(isStaleChunkError(new TypeError('Failed to fetch dynamically imported module: https://app/static/CollectionView-CFCv3Uid.js'))).toBe(true)
    expect(isStaleChunkError(new TypeError('Importing a module script failed.'))).toBe(true)
    expect(isStaleChunkError(new Error('Request failed with status code 500'))).toBe(false)
  })

  it('reloads into the target page once, not in a loop', () => {
    vi.useFakeTimers()
    expect(reloadForNewBuild('/acme/admin/branding')).toBe(true)
    expect(assign).toHaveBeenCalledWith('/acme/admin/branding')
    // The fresh page failing again straight away means the file is really gone.
    expect(reloadForNewBuild('/acme/admin/branding')).toBe(false)
    vi.advanceTimersByTime(11_000)
    expect(reloadForNewBuild('/acme/admin/branding')).toBe(true)
  })
})
