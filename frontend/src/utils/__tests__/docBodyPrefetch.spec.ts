import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import * as pool from '@nosdesk/core/sync/pool'

const WS_UUID = '0b6c3f0e-9a1d-4c2e-8f7a-5d4e3c2b1a09'
const PAGE_UUID = '6f1c2b8e-4d3a-4f5b-9c7d-2e1a0b9c8d7f'

const seed = vi.hoisted(() => ({ prefetchSavedCopy: vi.fn() }))
vi.mock('@/utils/collabSeed', () => seed)
const sessions = vi.hoisted(() => ({ open: new Set<string>() }))
vi.mock('@/stores/collabSession', () => ({
  useCollabSessionStore: () => ({
    peek: (docId: string) => (sessions.open.has(docId) ? {} : null),
  }),
}))
vi.mock('@/stores/myWorkspaces', () => ({
  useMyWorkspacesStore: () => ({ activeWorkspace: { workspace_uuid: WS_UUID } }),
}))

import { pageUuidForPath, prefetchPageBody } from '../docBodyPrefetch'

beforeEach(() => {
  setActivePinia(createPinia())
  pool.upsert('documentation_page', 5, { id: 5, uuid: PAGE_UUID, slug: 'payroll-runbook' })
})

afterEach(() => {
  pool.reset()
  sessions.open.clear()
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

  it('fetches nothing when a session already holds the page', () => {
    sessions.open.add(`ws-${WS_UUID}_doc-${PAGE_UUID}`)
    prefetchPageBody(PAGE_UUID)
    expect(seed.prefetchSavedCopy).not.toHaveBeenCalled()
  })
})
