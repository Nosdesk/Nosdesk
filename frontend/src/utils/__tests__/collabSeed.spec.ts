import { beforeEach, describe, expect, it, vi } from 'vitest'

const api = vi.hoisted(() => ({
  calls: [] as Array<{ url: string; params?: Record<string, unknown> }>,
  answer: (): Promise<{ data: { yjs_document?: string | null } }> =>
    Promise.resolve({ data: { yjs_document: btoa('\u0001\u0002') } }),
}))
vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    get: (url: string, config?: { params?: Record<string, unknown> }) => {
      api.calls.push({ url, params: config?.params })
      return api.answer()
    },
  },
}))

import { prefetchSavedCopy, takeSavedCopy } from '../collabSeed'
import { EDITOR_SCHEMA_VERSION } from '@/components/editor/schemaVersion'

beforeEach(() => {
  api.calls = []
  api.answer = () => Promise.resolve({ data: { yjs_document: btoa('\u0001\u0002') } })
})

describe("a page's saved copy", () => {
  it("is asked for with this editor's schema, once, whether prefetched or not", async () => {
    prefetchSavedCopy('page-a')
    prefetchSavedCopy('page-a')
    expect(await takeSavedCopy('page-a')).toEqual(new Uint8Array([1, 2]))
    expect(api.calls).toEqual([
      { url: '/documentation/pages/uuid/page-a/content', params: { schema: EDITOR_SCHEMA_VERSION } },
    ])

    // Taken, so the next open asks again.
    await takeSavedCopy('page-a')
    expect(api.calls).toHaveLength(2)
  })

  it('is nothing when the server withholds it or the request fails', async () => {
    api.answer = () => Promise.resolve({ data: { yjs_document: null } })
    expect(await takeSavedCopy('page-b')).toBeNull()
    api.answer = () => Promise.reject(new Error('offline'))
    expect(await takeSavedCopy('page-b')).toBeNull()
  })
})
