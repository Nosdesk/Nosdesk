import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import * as pool from '@nosdesk/core/sync/pool'

const WS_UUID = '0b6c3f0e-9a1d-4c2e-8f7a-5d4e3c2b1a09'
const PAGE_UUID = '6f1c2b8e-4d3a-4f5b-9c7d-2e1a0b9c8d7f'

const seed = vi.hoisted(() => ({ prefetchSavedCopy: vi.fn() }))
vi.mock('@/utils/collabSeed', () => seed)
// Docs whose session follows its room (or has a copy on its way), as the
// session store reports them.
const sessions = vi.hoisted(() => ({ current: new Set<string>(), warm: vi.fn() }))
vi.mock('@/stores/collabSession', () => ({
  useCollabSessionStore: () => ({
    needsSavedCopy: (docId: string) => !sessions.current.has(docId),
    warm: sessions.warm,
  }),
}))
vi.mock('@/stores/myWorkspaces', () => ({
  useMyWorkspacesStore: () => ({ activeWorkspace: { workspace_uuid: WS_UUID } }),
}))

import { pageUuidForPath, prefetchPageBody, warmPage } from '../docBodyPrefetch'

beforeEach(() => {
  setActivePinia(createPinia())
  pool.upsert('documentation_page', 5, { id: 5, uuid: PAGE_UUID, slug: 'payroll-runbook' })
})

afterEach(() => {
  pool.reset()
  sessions.current.clear()
  vi.clearAllMocks()
})

describe('the page a documentation path names', () => {
  it('is found in the docs pool by slug, or by id for a numeric path', () => {
    expect(pageUuidForPath('payroll-runbook')).toBe(PAGE_UUID)
    expect(pageUuidForPath('5')).toBe(PAGE_UUID)
  })

  it('is unknown when the pool does not hold it', () => {
    expect(pageUuidForPath('onboarding')).toBeNull()
    expect(pageUuidForPath('6')).toBeNull()
    expect(pageUuidForPath('')).toBeNull()
  })
})

describe("prefetching a page's body", () => {
  it('fetches the saved copy for a page this tab has no session for', () => {
    prefetchPageBody(PAGE_UUID)
    expect(seed.prefetchSavedCopy).toHaveBeenCalledWith(PAGE_UUID)
  })

  it('fetches nothing when a session already follows the page', () => {
    sessions.current.add(`ws-${WS_UUID}_doc-${PAGE_UUID}`)
    prefetchPageBody(PAGE_UUID)
    expect(seed.prefetchSavedCopy).not.toHaveBeenCalled()
  })
})

describe("warming a page's session from a link to it", () => {
  const DOC_ID = `ws-${WS_UUID}_doc-${PAGE_UUID}`

  it('opens the session for a page named by its uuid, or by an id the pool knows', () => {
    warmPage({ uuid: PAGE_UUID })
    warmPage({ id: 5 })
    warmPage({ id: '5' })
    expect(sessions.warm.mock.calls).toEqual([[DOC_ID], [DOC_ID], [DOC_ID]])
  })

  it("opens nothing for a page it can't name", () => {
    warmPage({ id: 6 })
    warmPage({})
    warmPage({ uuid: 'not-a-uuid' })
    expect(sessions.warm).not.toHaveBeenCalled()
  })
})
